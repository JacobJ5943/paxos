use tracing::{info, instrument};

use crate::{acceptors::Acceptor, PromiseReturn, SendToAcceptors};

#[derive(Debug)]
pub struct Proposer {
    pub current_highest_ballot: usize,
    pub node_identifier: usize,
}

impl Proposer {
    fn send_promise_messages() {}

    fn send_accept_messages() {}

    /// .
    /// TODO acceptors should not be this.
    /// # Errors
    ///
    /// This function will return an error if there is already an accepted value.  The value of the error will be that accepted value.
    #[instrument(skip(send_to_acceptors))]
    pub async fn propose_value(
        &mut self,
        initial_proposed_value: usize,
        send_to_acceptors: & impl SendToAcceptors,
        acceptor_identifiers: &mut Vec<usize>,
        total_acceptor_count: usize,
    ) -> Result<(),usize> {
        info!("Proposing_value");
        
        // This is mut for the case where an acceptor has already accepted a value
        let mut proposing_value = initial_proposed_value;

        let workign_promise_response: Option<PromiseReturn> = None;

        let mut acceptor_responses = Vec::new();
        for acceptor_id in acceptor_identifiers.iter() {
            acceptor_responses.push(
                send_to_acceptors
                    .send_promise(
                        *acceptor_id,
                        self.current_highest_ballot + 1,
                        self.node_identifier,
                    )
                    .await,
            );
        }

        let mut working_accepted_value = None;
        let mut highest_ballot_for_accepted_value = None;
        let mut promise_again: Vec<Acceptor> = vec![];
        // This is separate so that I can take the value of the acceptor with the highest ballot num
        // no real reason other than having determinism for breaking ties in this case
        // I'm sure this will change when I move to async.
        let mut highest_ballot_no_value = None;

        //  What really is the okay count?
        // What if there is only one acceptor that has yet to accept a value
        // How do I deal with the okay count there and what does it represent?
        let mut ok_count = 0;
        // I don't like the enumerator index keeping track of which acceptor this is
        // This won't translate well to when these are async
        for (acceptor_index, response) in acceptor_responses.iter().enumerate() {
            match response {
                Ok(_) => ok_count += 1,
                Err(error_response) => {
                    if error_response.highest_node_identifier == self.current_highest_ballot {
                        // this is a really odd case, but easy to handle I guess. Just increase the self highest ballot count.  No promise needed

                        if error_response.accepted_value.is_some() {
                            match highest_ballot_for_accepted_value {
                                Some(highest_ballot_so_far) => {
                                    ok_count += 1; // after changing highest_ballot_num this becomes an okay response
                                    if error_response.highest_ballot_num > highest_ballot_so_far {
                                        working_accepted_value = error_response.accepted_value;
                                        highest_ballot_for_accepted_value =
                                            Some(error_response.highest_ballot_num);
                                    }
                                }
                                None => {
                                    ok_count += 1; // after changing highest_ballot_num this becomes an okay response
                                    working_accepted_value = error_response.accepted_value;
                                    highest_ballot_for_accepted_value =
                                        Some(error_response.highest_ballot_num);
                                }
                            }
                        } else {
                            // I don't know if I should be overwriting this value, but it seems to not matter
                            if let Some(high_ballot) = highest_ballot_no_value {
                                if error_response.highest_ballot_num > high_ballot {
                                    highest_ballot_no_value =
                                        Some(error_response.highest_ballot_num);
                                }
                            } else {
                                highest_ballot_no_value = Some(error_response.highest_ballot_num);
                            }
                        }
                    } else {
                        match error_response.accepted_value {
                            Some(accpeted_value) => {
                                // This is the easy case.  We don't need to send another promise because it won't accept the value anyway

                                match highest_ballot_for_accepted_value {
                                    Some(highest_ballot_so_far) => {
                                        if error_response.highest_ballot_num > highest_ballot_so_far
                                        {
                                            working_accepted_value = error_response.accepted_value;
                                            highest_ballot_for_accepted_value =
                                                Some(error_response.highest_ballot_num);
                                        }
                                    }
                                    None => {
                                        working_accepted_value = error_response.accepted_value;
                                        highest_ballot_for_accepted_value =
                                            Some(error_response.highest_ballot_num);
                                    }
                                }
                            }
                            None => {
                                todo!()
                                // This means that we need to send out another promise to this node if we want to send an accept
                            }
                        }
                    }
                }
            }
        }

        for acceptor_index in promise_again {
            // Send that promise with the new highest ballot count
            todo!()
        }

        if let Some(high_ballot) = dbg!(highest_ballot_for_accepted_value) {
            if high_ballot > self.current_highest_ballot {
                self.current_highest_ballot = high_ballot;
            }
        };
        if let Some(high_ballot) = dbg!(highest_ballot_no_value) {
            if high_ballot > self.current_highest_ballot {
                self.current_highest_ballot = high_ballot;
            }
        }

        if let Some(accepted_value) = working_accepted_value {
            proposing_value = accepted_value;
        }

        dbg!(self.current_highest_ballot);


        let mut accepted_count = 0;
        // this is the error with main
        for acceptor_id in acceptor_identifiers.iter() {
            /*let accept_result = send_to_acceptors
                     .send_accept(
                        *acceptor_id,
                        proposing_value,
                        self.current_highest_ballot + 1,
                        self.node_identifier,
                    )
                    .await.is_ok();
                    if accept_result {
                        accepted_count += 1;
                    }*/
            //accept_responses.push(
                //accept_result
            //);
        }

        if accepted_count >= (total_acceptor_count / 2) + 1 {
            // This means that the majority have accepted this value and it has been decided
        }

        // This doesn't account for if there was not a majority of acceptors
        if proposing_value != initial_proposed_value {
            Err(proposing_value)
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
mod prop_tests {

    struct LocalSendToAcceptor {
        acceptors: Arc<Mutex<Vec<Acceptor>>>,
    }
    impl LocalSendToAcceptor {
        fn new(acceptors: Arc<Mutex<Vec<Acceptor>>>) -> Self {
            Self { acceptors }
        }
    }

    #[async_trait]
    impl SendToAcceptors for LocalSendToAcceptor {
        async fn send_accept(
            &self,
            acceptor_identifier: usize,
            value: usize,
            ballot_num: usize,
            proposer_identifier: usize,
        ) -> Result<(),()> {
            self.acceptors.lock().unwrap()[acceptor_identifier].accept(ballot_num, proposer_identifier, value)
        }

        async fn send_promise(
            &self,
            acceptor_identifier: usize,
            ballot_num: usize,
            proposer_identifer: usize,
        ) -> Result<(), PromiseReturn> {
            self.acceptors.lock().unwrap()[acceptor_identifier].promise(ballot_num, proposer_identifer)
        }
    }

    use std::{cell::RefCell, rc::Rc, sync::{Mutex, Arc}};

    use async_trait::async_trait;

    use crate::{acceptors::Acceptor, proposers::Proposer, SendToAcceptors, PromiseReturn};

    // IMPORTANT TODO look into tracing-test crate
    #[test]
    fn install_subscriber() {
        let my_sub = tracing_subscriber::FmtSubscriber::new();
        tracing::subscriber::set_global_default(my_sub).expect("This should not fail");
    }

    #[test]
    fn testing_usize_division() {
        assert_eq!(3 / 2, 1);
        assert_eq!((3 / 2) + 1, 2);
        assert_eq!((4 / 2) + 1, 3);
    }
    #[test]
    fn it_works() {
        let result = 2 + 2;
        assert_eq!(result, 4);
    }

    #[tokio::test]
    async fn test_only_one_proposer() {
        let mut acceptors = vec![
            Acceptor::default(),
            Acceptor::default(),
            Acceptor::default(),
        ];

        let mut local_sender = LocalSendToAcceptor::new(Arc::new(Mutex::new(acceptors)));

        let mut proposer = Proposer {
            current_highest_ballot: 0,
            node_identifier: 0,
        };

        let result = proposer.propose_value(5,&local_sender, &mut vec![0,1,2], 5).await;

        assert!(result.is_ok());
        // Currently this is manually being checked by looking at the tracing output
        // This should be automated by tracing_test, but first get some tests out and debug
        //todo!();
    }

    #[tokio::test]
    async fn test_proposer_a_sends_2_to_two_then_proposer_b_sends_3_to_all_3() {
        let mut acceptors = vec![Acceptor::default(), Acceptor::default(), Acceptor::default()];
        let mut local_sender = LocalSendToAcceptor::new(Arc::new(Mutex::new(acceptors)));
        let mut acceptors = vec![0,1];

        let mut proposer_a = Proposer {
            current_highest_ballot: 0,
            node_identifier: 0,
        };

        let mut proposer_b = Proposer {
            current_highest_ballot: 0,
            node_identifier: 1,
        };

        let prop_a_result = proposer_a.propose_value(2,&local_sender, &mut acceptors, 3).await;
        acceptors.push(2);
        let prop_b_result = proposer_b.propose_value(5, &local_sender, &mut acceptors, 3).await;

        assert!(prop_a_result.is_ok());
        assert!(prop_b_result.is_err());
        assert_eq!(prop_b_result.unwrap_err(), 2);
    }

    // I'm not totally sure what I want form this test
    // Currently if the acceptors are promised to a proposer.  If a proposer sends a higher ballot number they will accept it.
    // This means after the proposer has sent out all of the promises it can just send an accept with the higher number
    // so the acceptors should accept the value, but might not change their highest ballot count.
    #[tokio::test]
    async fn test_take_highest_value_ballot_count() {
        // I should probably set the node identifier so I don't rely on the default?
        let mut acceptors = vec![
            Acceptor::default(),
            Acceptor::default(),
            Acceptor::default(),
        ];
        acceptors[0].promised_ballot_num = Some(5);
        acceptors[1].promised_ballot_num = Some(7);
        acceptors[2].promised_ballot_num = Some(4);


        let mut local_sender = LocalSendToAcceptor::new(Arc::new(Mutex::new(acceptors)));
        let mut acceptors = vec![0,1,2];

        let mut proposer_a = Proposer {
            current_highest_ballot: 0,
            node_identifier: 0,
        };

        let prop_a_result = proposer_a.propose_value(2,&local_sender, &mut acceptors, 3).await;
        assert_eq!(proposer_a.current_highest_ballot, 7);

        assert_eq!(local_sender.acceptors.lock().unwrap()[0].accepted_value, Some(2));
        assert_eq!(local_sender.acceptors.lock().unwrap()[1].accepted_value, Some(2));
        assert_eq!(local_sender.acceptors.lock().unwrap()[2].accepted_value, Some(2));
        //assert_eq!(acceptors[0].promised_ballot_num,Some(8));
        //assert_eq!(acceptors[1].promised_ballot_num,Some(8));
        //assert_eq!(acceptors[2].promised_ballot_num,Some(8));
    }
}
