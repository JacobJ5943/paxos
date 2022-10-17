use futures::{future, select, StreamExt};
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
        send_to_acceptors: &impl SendToAcceptors,
        acceptor_identifiers: &mut Vec<usize>,
        total_acceptor_count: usize,
    ) -> Result<(), usize> {
        info!("Proposing_value");

        // This is mut for the case where an acceptor has already accepted a value
        let mut proposing_value = initial_proposed_value;

        let workign_promise_response: Option<PromiseReturn> = None;

        let mut promise_futures = Vec::new();
        self.current_highest_ballot += self.current_highest_ballot + 1;
        for acceptor_id in acceptor_identifiers.iter() {
            promise_futures.push(send_to_acceptors.send_promise(
                *acceptor_id,
                self.current_highest_ballot,
                self.node_identifier,
            ));
        }

        let mut futures_unsorted =
            futures::stream::FuturesUnordered::from_iter(promise_futures.into_iter());

        let quarem = ((acceptor_identifiers.len() as f64 / 2.0).ceil()) as usize;

        let mut results_returned = Vec::new();

        // Keep pulling from futures_unsorted until I have a majority of responses.
        // From what I remember I need to wait for a majority of responses before I do some processing
        for _ in 0..quarem {
            let result = futures_unsorted.select_next_some().await;
            results_returned.push(result);
        }

        // Once I have a majority of responses I am going to take the value of the highest ballot num returned.
        // I'm not sure if I need to take the value of a different one if the highest ballot num does not have a value.
        // for now I am taking the highest ballot num as is
        let working_ballot = results_returned
            .iter()
            .reduce(|acc, next| {
                if acc.is_ok() {
                    next
                } else {
                    match (acc, next) {
                        (Err(acc_err), Ok(())) => acc,
                        (Err(acc_err), Err(next_err)) => {
                            if (next_err.highest_ballot_num > acc_err.highest_ballot_num)
                                || (next_err.highest_ballot_num == acc_err.highest_ballot_num
                                    && next_err.highest_node_identifier
                                        > acc_err.highest_node_identifier)
                            {
                                next
                            } else {
                                acc
                            }
                        }
                        (_, _) => {
                            // Acc is not ok
                            unreachable!()
                        }
                    }
                }
            })
            .unwrap(); // quarem must be greater than 0 as such there must be greater than 0 items in results_returned

        // take the highest ballot num.  If it has a value then set the propsing value to that
        match working_ballot {
            Ok(()) => { // This means we are good to go
            }
            Err(promise_return) => {
                // Now the accepts should pass.
                // This is where more promises may need to be sent with this ballot number
                self.current_highest_ballot = promise_return.highest_ballot_num + 1;
                if let Some(accepted_value) = promise_return.accepted_value {
                    proposing_value = accepted_value;
                }
            }
        };
        // If the there was not a value we will keep trying the proposing value.
        // If the propsing value is accepted then this will forever be the proposing value.
        // If a value is received from a failed promise/accept then all subsequent promise/accept will contain this new value

        // I don't know what to do about the promise accept for each
        // I'm thinking i have them all in the fused futures and then keep popping the newest
        // If accept passed great.  If not create another promise then accept phase for that 
        // Could probably encode that as an enum for the type state pattern

        // Then once a majority of acceptors has accepted a single value that's our answer and keep propegating that
        // 

        let mut accepted_count = 0;
        // this is the error with main
        for acceptor_id in acceptor_identifiers.iter() {
            let accept_result = send_to_acceptors
                .send_accept(
                    *acceptor_id,
                    proposing_value,
                    self.current_highest_ballot + 1,
                    self.node_identifier,
                )
                .await
                .is_ok();
            if accept_result {
                accepted_count += 1;
            }
            //accept_responses.push(
            //accept_result
            //);
        }

        if accepted_count >= quarem {
            // decided value
        }

        if proposing_value != initial_proposed_value {
            Err(proposing_value)
        } else {
            Ok(())
        }

        // I guess the big question is how do I handle situations where there is latency and responses don't come in at the same time
        // or if there is contention
        // Do I send out new promises to everyone with a new higher ballot?  Just that acceptor?  Do I update my ballot num and just keep using that and updating that.  Id on't think I will need to keep doing the same ballot nubmer for each acceptor
        // Although I could just use the response from each acceptor and have a ballot number specific to each acceptor
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
        ) -> Result<(), ()> {
            self.acceptors.lock().unwrap()[acceptor_identifier].accept(
                ballot_num,
                proposer_identifier,
                value,
            )
        }

        async fn send_promise(
            &self,
            acceptor_identifier: usize,
            ballot_num: usize,
            proposer_identifer: usize,
        ) -> Result<(), PromiseReturn> {
            self.acceptors.lock().unwrap()[acceptor_identifier]
                .promise(ballot_num, proposer_identifer)
        }
    }

    use std::{
        cell::RefCell,
        rc::Rc,
        sync::{Arc, Mutex},
    };

    use async_trait::async_trait;

    use crate::{acceptors::Acceptor, proposers::Proposer, PromiseReturn, SendToAcceptors};

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

        let result = proposer
            .propose_value(5, &local_sender, &mut vec![0, 1, 2], 5)
            .await;

        assert!(result.is_ok());
        // Currently this is manually being checked by looking at the tracing output
        // This should be automated by tracing_test, but first get some tests out and debug
        //todo!();
    }

    #[tokio::test]
    async fn test_proposer_a_sends_2_to_two_then_proposer_b_sends_3_to_all_3() {
        let mut acceptors = vec![
            Acceptor::default(),
            Acceptor::default(),
            Acceptor::default(),
        ];
        let mut local_sender = LocalSendToAcceptor::new(Arc::new(Mutex::new(acceptors)));
        let mut acceptors = vec![0, 1];

        let mut proposer_a = Proposer {
            current_highest_ballot: 0,
            node_identifier: 0,
        };

        let mut proposer_b = Proposer {
            current_highest_ballot: 0,
            node_identifier: 1,
        };

        let prop_a_result = proposer_a
            .propose_value(2, &local_sender, &mut acceptors, 3)
            .await;
        acceptors.push(2);
        let prop_b_result = proposer_b
            .propose_value(5, &local_sender, &mut acceptors, 3)
            .await;

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
        let mut acceptors = vec![0, 1, 2];

        let mut proposer_a = Proposer {
            current_highest_ballot: 0,
            node_identifier: 0,
        };

        let prop_a_result = proposer_a
            .propose_value(2, &local_sender, &mut acceptors, 3)
            .await;
        assert_eq!(proposer_a.current_highest_ballot, 8);

        assert_eq!(
            local_sender.acceptors.lock().unwrap()[0].accepted_value,
            Some(2)
        );
        assert_eq!(
            local_sender.acceptors.lock().unwrap()[1].accepted_value,
            Some(2)
        );
        assert_eq!(
            local_sender.acceptors.lock().unwrap()[2].accepted_value,
            Some(2)
        );
        //assert_eq!(acceptors[0].promised_ballot_num,Some(8));
        //assert_eq!(acceptors[1].promised_ballot_num,Some(8));
        //assert_eq!(acceptors[2].promised_ballot_num,Some(8));
    }
}
