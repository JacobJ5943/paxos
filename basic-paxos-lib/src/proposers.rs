use std::collections::HashMap;

use futures::stream::FuturesUnordered;
use futures::{future, select, StreamExt};
use tracing::{info, instrument};

use crate::{
    acceptors::{AcceptedValue, Acceptor, HighestBallotPromised},
    PromiseReturn, SendToAcceptors,
};

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
        let mut a_value_has_been_accepted = false; // This should really be propsing_value as an option or something



        let workign_promise_response: Option<PromiseReturn> = None;

        let mut sending_promises = Vec::new();

        self.current_highest_ballot += self.current_highest_ballot + 1;
        for acceptor_id in acceptor_identifiers.iter() {
            sending_promises.push(
                AwaitingPromise {
                    acceptor_id: *acceptor_id,
                }
                .send_promise(self.current_highest_ballot, self.node_identifier, send_to_acceptors),
            );
        }

        let mut promise_futures_unsorted =
            futures::stream::FuturesUnordered::from_iter(sending_promises.into_iter());

        let mut accept_futures_unsorted = futures::stream::FuturesUnordered::new();

        let quarem = ((acceptor_identifiers.len() as f64 / 2.0).floor()) as usize + 1;

        let mut results_returned = Vec::new();

        // Keep pulling from futures_unsorted until I have a majority of responses.
        // From what I remember I need to wait for a majority of responses before I do some processing
        for _ in 0..quarem {
            let result = promise_futures_unsorted.select_next_some().await;
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
                        (Err((_, acc_promise_return)), Ok(_)) => acc,
                        (Err((_, acc_promise_return)), Err((_, next_promise_return))) => {
                            if (next_promise_return.highest_ballot_num
                                > acc_promise_return.highest_ballot_num)
                                || (next_promise_return.highest_ballot_num
                                    == acc_promise_return.highest_ballot_num
                                    && next_promise_return.highest_node_identifier
                                        > acc_promise_return.highest_node_identifier)
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
            Ok(_awaiting_accept) => { // This means we are good to go
            }
            Err((_awaiting_promise, promise_return)) => {
                // Now the accepts should pass.
                // This is where more promises may need to be sent with this ballot number
                self.current_highest_ballot = promise_return.highest_ballot_num + 1;
                if let Some(accepted_value) = promise_return.accepted_value {
                    proposing_value = accepted_value;
                    a_value_has_been_accepted = true;
                }
            }
        };

        for awaiting_result in results_returned.into_iter() {
            match awaiting_result {
                Ok(awaiting_accept) => {
                    accept_futures_unsorted.push(awaiting_accept.send_accept(
                        proposing_value,
                        self.current_highest_ballot,
                        self.node_identifier,
                        send_to_acceptors
                    ));
                }
                Err((awaiting_promise, promise_return)) => {
                    promise_futures_unsorted.push(
                        awaiting_promise
                            .send_promise(self.current_highest_ballot, self.node_identifier, send_to_acceptors),
                    );
                }
            }
        }

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

        // <AccpetedValue, count>
        let mut accepted_results: HashMap<usize, usize> = HashMap::new();

        let mut decided_value: Option<usize> = None;

        //let x: Result<AwaitingAccept, (AwaitingPromise, PromiseReturn)> =
        //promise_futures_unsorted.select_next_some().await;
        // let y: Result<Result<usize, AcceptedValue>, (AwaitingPromise, HighestBallotPromised)> =
        // accept_futures_unsorted.select_next_some().await;
        while !promise_futures_unsorted.is_empty() || !accept_futures_unsorted.is_empty() {
            // Formatting is a reason to hate macros
            // a big reason
            select! {
                promise_response = promise_futures_unsorted.select_next_some() => {
                    info!("we just got a promise_response {:?}, ({:?},{:?})->{:?},{:?}", &promise_response, promise_futures_unsorted.len(), accept_futures_unsorted.len(), accepted_results, acceptor_identifiers);
                // Holy shit this is so gross without typing in a macro
                    match promise_response {
                        Ok(awaiting_accept) => {
                            accept_futures_unsorted.push(awaiting_accept.send_accept(proposing_value, self.current_highest_ballot, self.node_identifier, send_to_acceptors))
                        },
                        Err((awaiting_promise, promise_return)) =>  {
                            // need a self
                            self.current_highest_ballot = (promise_return.highest_ballot_num + 1).max(self.current_highest_ballot);
                            promise_futures_unsorted.push(awaiting_promise.send_promise(self.current_highest_ballot, self.node_identifier, send_to_acceptors));
                        }
                    }
                },

                accept_response = accept_futures_unsorted.select_next_some() => {
                    info!("we just got a accept_response {:?}({:?},{:?}", &accept_response, promise_futures_unsorted.len(), accept_futures_unsorted.len());
                    match accept_response {
                        Ok(value_accepted) => {
                            let value_accepted = match value_accepted {
                                Ok(va) => va,
                                Err(av) => av.0
                            };

                            // I don't like this
                            if !a_value_has_been_accepted {
                                proposing_value = value_accepted;
                                a_value_has_been_accepted = true;
                            }

                            match accepted_results.entry(value_accepted) {
                                std::collections::hash_map::Entry::Occupied(mut oc) => {*oc.into_mut() +=1;},
                                std::collections::hash_map::Entry::Vacant(mut vac) => {vac.insert(1);},
                            }
                            if accepted_results.get(&value_accepted).unwrap() >= &quarem {
                                decided_value = Some(value_accepted);
                                break;
                                // This value has been accepted
                            }
                        },
                        Err((awaiting_promise, highest_ballot_promised)) => {
                            self.current_highest_ballot = self.current_highest_ballot.max(highest_ballot_promised.0.expect("Being rejected after a promise so a value must be accepted") + 1);
                            promise_futures_unsorted.push(awaiting_promise.send_promise(self.current_highest_ballot,self.node_identifier, send_to_acceptors))
                        },
                    }

                }
            }

            if accepted_count >= quarem {
                // This should actually never occur
                unreachable!("This should never be reached because we always check for the quarem after each update of the accepted results");
            }
        }

        if decided_value.is_none() {
            panic!("some how we ran out of accept results without reaching a quarem.  If this ever occurs I would think it has to do with how I am proposing the value if one already exists")
        }
        if decided_value.unwrap() != initial_proposed_value {
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

// Is that the right future type?

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
        ) -> Result<AcceptedValue, HighestBallotPromised> {
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

    use crate::{
        acceptors::{AcceptedValue, Acceptor, HighestBallotPromised},
        proposers::Proposer,
        PromiseReturn, SendToAcceptors,
    };

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
    // This test will never nhault because We won't have a quarem and so the prop_a_result will never resolve
    // i'm not sure how I will write this test
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



struct AwaitingPromise {
    acceptor_id: usize,
}

impl std::fmt::Debug for AwaitingPromise {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AwaitingPromise").field("acceptor_id", &self.acceptor_id).finish()
    }
}

impl AwaitingPromise {
    async fn send_promise(
        self: AwaitingPromise,
        ballot_num: usize,
        proposer_identifier: usize,
        send_to_acceptors:  &impl SendToAcceptors
    ) -> Result<AwaitingAccept, (AwaitingPromise, PromiseReturn)> {
        let result = send_to_acceptors
            .send_promise(self.acceptor_id, ballot_num, proposer_identifier)
            .await;

        match result {
            Ok(_) => Ok(AwaitingAccept {
                acceptor_id: self.acceptor_id,
            }),
            Err(promise_return) => Err((self, promise_return)),
        }
    }
}
struct AwaitingAccept {
    acceptor_id: usize,
}

impl std::fmt::Debug for AwaitingAccept {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AwaitingAccept").field("acceptor_id", &self.acceptor_id).finish()
    }
}

impl AwaitingAccept {
    async fn send_accept(
        self: AwaitingAccept,
        proposing_value: usize,
        ballot_num: usize,
        proposer_identifier: usize,
        send_to_acceptors:&impl SendToAcceptors
    ) -> Result<Result<usize, AcceptedValue>, (AwaitingPromise, HighestBallotPromised)> {
        let result = send_to_acceptors
            .send_accept(
                self.acceptor_id,
                proposing_value,
                ballot_num,
                proposer_identifier,
            )
            .await;

        match result {
            Ok(ok_result) => {
                // This means that the acceptor has already accepted a value for this slot so no progress can be made
                if ok_result.0 == proposing_value {
                    Ok(Ok(proposing_value))
                } else {
                    Ok(Err(ok_result))
                }
            }
            Err(highest_ballot_promised) => Err((
                AwaitingPromise {
                    acceptor_id: self.acceptor_id,
                },
                highest_ballot_promised,
            )),
        }
    }
}
