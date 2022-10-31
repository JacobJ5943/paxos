use std::collections::HashMap;

use futures::StreamExt;
use tracing::{info, instrument};

use crate::{
    acceptors::{AcceptedValue, HighestBallotPromised},
    PromiseReturn, SendToAcceptors,
};

#[derive(Debug)]
pub struct Proposer {
    pub current_highest_ballot: usize,
    pub node_identifier: usize,
}

impl Proposer {
    /// .
    /// Sends the propose message to all acceptors specified by send_to_acceptors.
    /// Returns Ok if all promises returned Ok(None)
    ///
    /// Else Returns Err with the highest ballot number of all responses and the AcceptedValue with the highest matching ballot num.
    ///
    ///
    /// # Errors
    /// Returns
    /// 1. The highest ballot number out of all the responses received
    /// 2. The AcceptedValue which corresponds the highest ballot num of all the received responses that contain a AcceptedValue.
    ///
    /// This function will return an error if .
    #[instrument(skip(send_to_acceptors))]
    async fn promise_quorum(
        &mut self,
        acceptor_identifiers: &mut Vec<usize>,
        send_to_acceptors: &impl SendToAcceptors,
    ) -> Result<(), (HighestBallotPromised, Option<AcceptedValue>)> {
        let mut sending_promises = Vec::new();

        self.current_highest_ballot += 1;
        for acceptor_id in acceptor_identifiers.iter() {
            sending_promises.push(
                AwaitingPromise {
                    acceptor_id: *acceptor_id,
                }
                .send_promise(
                    self.current_highest_ballot,
                    self.node_identifier,
                    send_to_acceptors,
                ),
            );
        }

        let mut promise_futures_unsorted =
            futures::stream::FuturesUnordered::from_iter(sending_promises.into_iter());

        // Get the first quorum results

        let quorum = ((acceptor_identifiers.len() as f64 / 2.0).floor()) as usize + 1;

        let mut results = Vec::new();
        while results.len() < quorum && !promise_futures_unsorted.is_empty() {
            let new_result = promise_futures_unsorted.select_next_some().await;
            results.push(new_result);
        }

        let mut highest_ballot_for_value: Option<usize> = None;

        let mut working_promise: Option<PromiseReturn> = None;
        for result in results.into_iter() {
            match (result, &mut working_promise) {
                (Ok(accepted_value), None) => {
                    if let Some(accepted_value) = accepted_value {
                        working_promise = Some(PromiseReturn {
                            highest_ballot_num: self.current_highest_ballot,
                            highest_node_identifier: self.node_identifier,
                            accepted_value: Some(accepted_value).map(|av| av.0),
                        });
                    }
                }
                (Ok(accepted_value), Some(working_promise)) => {
                    if working_promise.accepted_value.is_none() && accepted_value.is_some() {
                        working_promise.accepted_value = accepted_value.map(|av| av.0);
                    }
                } // This is just a nop right?  because the promise was good?
                (Err(result_promise_return), Some(working_promise)) => {
                    if result_promise_return.highest_ballot_num > working_promise.highest_ballot_num
                    {
                        working_promise.highest_ballot_num =
                            result_promise_return.highest_ballot_num;
                    }

                    if result_promise_return.accepted_value.is_some()
                        && (highest_ballot_for_value.is_none()
                            || result_promise_return.highest_ballot_num
                                > highest_ballot_for_value.unwrap())
                    {
                        highest_ballot_for_value = Some(result_promise_return.highest_ballot_num);
                        working_promise.accepted_value = result_promise_return.accepted_value;
                    }
                }
                (Err(result_promise_return), None) => {
                    working_promise = Some(result_promise_return);
                }
            }
        }

        match working_promise {
            None => {
                // There has not been a decided value and we're good to go
                Ok(())
            }
            Some(working_promise_err) => {
                // This is so gosh darn verbose gross
                Err((
                    HighestBallotPromised(Some(working_promise_err.highest_ballot_num)),
                    working_promise_err.accepted_value.map(AcceptedValue),
                ))
            }
        }
    }

    ///
    /// Sends an accept message to all acceptors.  After a majority of acceptors have responded with a single value it is decided and Ok(<decided_value>) is returned.
    /// If every acceptor has responded and a value has not been decided Err(()) is returned.  It's up to the caller to retry
    ///
    ///
    /// Errors on case of no value decided
    #[instrument(skip(send_to_acceptors))]
    async fn accept_quorum(
        &mut self,
        proposing_value: usize,
        acceptor_identifiers: &mut Vec<usize>,
        send_to_acceptors: &impl SendToAcceptors,
        quorum: usize,
    ) -> Result<usize, ()> {
        let mut sending_accepts = Vec::new();
        for acceptor_id in acceptor_identifiers.iter() {
            sending_accepts.push(
                AwaitingAccept {
                    acceptor_id: *acceptor_id,
                }
                .send_accept(
                    proposing_value,
                    self.current_highest_ballot,
                    self.node_identifier,
                    send_to_acceptors,
                ),
            );
        }
        let mut accept_futures_unsorted =
            futures::stream::FuturesUnordered::from_iter(sending_accepts.into_iter());

        let mut accepted_results = HashMap::new();
        let mut decided_value = None;
        while !accept_futures_unsorted.is_empty() {
            let accept_response = accept_futures_unsorted.select_next_some().await;
            match accept_response {
                Ok(value_accepted) => {
                    let value_accepted = match value_accepted {
                        Ok(va) => va,
                        Err(av) => av.0,
                    };

                    match accepted_results.entry(value_accepted) {
                        std::collections::hash_map::Entry::Occupied(oc) => {
                            *oc.into_mut() += 1;
                        }
                        std::collections::hash_map::Entry::Vacant(vac) => {
                            vac.insert(1);
                        }
                    }
                    if accepted_results.get(&value_accepted).unwrap() >= &quorum {
                        //return Ok(value_accepted);// figure out short circuiting later
                        // There was a test that requires all 3/3 acceptors to have accepted.  Only 2/3 did with the short circuit so while decided still failed
                        decided_value = Some(value_accepted)
                        // This value has been accepted
                    }
                }
                Err((_awaiting_promise, _highest_ballot_promised)) => {
                    // todo
                    // Should this have a todo!() macro?
                    // This would be if there was a competing proposer with the same ballot number
                }
            }
        }

        if let Some(decided_value) = decided_value {
            Ok(decided_value)
        } else {
            Err(())
        }
    }

    ///
    /// The function to propose a value.
    ///
    /// Returns Ok(()) if the proposed value is decided
    /// Returns Err(<decided_value>) if some other value is decided
    ///
    /// Panics if .
    ///
    /// # Errors
    ///
    /// This function will return an error if .
    pub async fn propose_value(
        &mut self,
        initial_proposed_value: usize,
        send_to_acceptors: &impl SendToAcceptors,
        acceptor_identifiers: &mut Vec<usize>,
        _total_acceptor_count: usize, // unused for now, but will probably be a config thing to allow for resizing.  This would determine the quorum based on the slot I suppose?
    ) -> Result<(), usize> {
        info!("Proposing_value");

        // This is mut for the case where an acceptor has already accepted a value
        let mut proposing_value = initial_proposed_value;

        let mut decided_value = Err(());

        while decided_value.is_err() {
            info!("Starting promise while loop.");
            while let Err((highest_ballot, accepted_value)) = dbg!(
                self.promise_quorum(acceptor_identifiers, send_to_acceptors)
                    .await
            ) {
                // Safety on unwrap
                // a promise response will always have a highest ballot or else the promise would succeed
                if self.current_highest_ballot >= highest_ballot.0.unwrap() {
                    if let Some(accepted_value) = &accepted_value {
                        if proposing_value == accepted_value.0 {
                            break;
                        }
                    }
                }

                // Safety on unwrap
                // a promise response will always have a highest ballot or else the promise would succeed
                self.current_highest_ballot = highest_ballot.0.unwrap() + 1;
                if let Some(accepted_value) = accepted_value {
                    proposing_value = accepted_value.0;
                }
            }

            let quorum = ((acceptor_identifiers.len() as f64 / 2.0).floor()) as usize + 1;

            decided_value = self
                .accept_quorum(
                    proposing_value,
                    acceptor_identifiers,
                    send_to_acceptors,
                    quorum,
                )
                .await;
            info!("Decided value for this loop is {:?}", decided_value);
        }

        match decided_value {
            Ok(decided_value) => {
                if decided_value != initial_proposed_value {
                    Err(proposing_value)
                } else {
                    Ok(())
                }
            }
            Err(_) => {
                // This is the case because if a value is not decided the while let loop above will not exit
                unreachable!()
            }
        }
    }
}

#[cfg(test)]
mod prop_tests {

    /*
     * TODO Add mutexes to control the flow of traffic
     *
     * I want to be able to emulate my man in the middle crate with mutexes or some other sync mechanism
     * This will allow me to write much more complex tests
     */
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
            proposer_identifier: usize,
        ) -> Result<Option<AcceptedValue>, PromiseReturn> {
            self.acceptors.lock().unwrap()[acceptor_identifier]
                .promise(ballot_num, proposer_identifier)
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
    // This test will never nhault because We won't have a quorum and so the prop_a_result will never resolve
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
        f.debug_struct("AwaitingPromise")
            .field("acceptor_id", &self.acceptor_id)
            .finish()
    }
}

impl AwaitingPromise {
    async fn send_promise(
        self: AwaitingPromise,
        ballot_num: usize,
        proposer_identifier: usize,
        send_to_acceptors: &impl SendToAcceptors,
    ) -> Result<Option<AcceptedValue>, PromiseReturn> {
        let result = send_to_acceptors
            .send_promise(self.acceptor_id, ballot_num, proposer_identifier)
            .await;

        match result {
            Ok(accepted_value) => Ok(accepted_value),
            Err(promise_return) => Err(promise_return),
        }
    }
}
struct AwaitingAccept {
    acceptor_id: usize,
}

impl std::fmt::Debug for AwaitingAccept {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AwaitingAccept")
            .field("acceptor_id", &self.acceptor_id)
            .finish()
    }
}

impl AwaitingAccept {
    async fn send_accept(
        self: AwaitingAccept,
        proposing_value: usize,
        ballot_num: usize,
        proposer_identifier: usize,
        send_to_acceptors: &impl SendToAcceptors,
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
