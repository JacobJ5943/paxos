use std::collections::HashMap;

use futures::stream::{select_all, FuturesUnordered};
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
    #[instrument(skip(send_to_acceptors))]
    async fn promise_quarem(
        &mut self,
        acceptor_identifiers: &mut Vec<usize>,
        send_to_acceptors: &impl SendToAcceptors,
    ) -> Result<(), (HighestBallotPromised, Option<AcceptedValue>)> {
        let mut sending_promises = Vec::new();

        self.current_highest_ballot = self.current_highest_ballot + 1;
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

        // Get the first quarem results

        let quarem = ((acceptor_identifiers.len() as f64 / 2.0).floor()) as usize + 1;

        let mut results = Vec::new();
        while results.len() < quarem && !promise_futures_unsorted.is_empty() {
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

    /*
     * I'm not really sure when to finish this funciton
     *
     * What I'm going to do for now because I need to decide something is to send the accept to everyone
     * Then process results until either a quarem is reached or all results have been processed
     *
     * I don't really know what happens when there are network failures
     *
     * here the result
     * Ok(decided_value)
     * Err(I'll decide what to do with retries later.  This stuff is complicated)
     */
    #[instrument(skip(send_to_acceptors))]
    async fn accept_quarem(
        &mut self,
        proposing_value: usize,
        acceptor_identifiers: &mut Vec<usize>,
        send_to_acceptors: &impl SendToAcceptors,
        quarem: usize,
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
                        std::collections::hash_map::Entry::Occupied(mut oc) => {
                            *oc.into_mut() += 1;
                        }
                        std::collections::hash_map::Entry::Vacant(mut vac) => {
                            vac.insert(1);
                        }
                    }
                    if accepted_results.get(&value_accepted).unwrap() >= &quarem {
                        //return Ok(value_accepted);// figure out short circuiting later
                        // There was a test that requires all 3/3 acceptors to have accepted.  Only 2/3 did with the short circuit so while decided still failed
                        decided_value = Some(value_accepted)
                        // This value has been accepted
                    }
                }
                Err((awaiting_promise, highest_ballot_promised)) => {
                    // todo
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

    fn send_promise_messages() {}

    fn send_accept_messages() {}

    /// .
    /// TODO acceptors should not be this.
    ///
    /// TODO The return type for if it was already accepted should not be if the value is different
    /// # Errors
    ///
    /// This function will return an error if there is already an accepted value.  The value of the error will be that accepted value.
    ///
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
        let mut accepted_results: HashMap<usize, usize> = HashMap::new();

        let workign_promise_response: Option<PromiseReturn> = None;

        let mut decided_value = Err(());

        while let Err(_) = decided_value {
            info!("Starting promise while loop.");
            while let Err((highest_ballot, accepted_value)) = dbg!(
                self.promise_quarem(acceptor_identifiers, send_to_acceptors)
                    .await
            ) {
                // Let's go more comment streams
                /*
                 * Here I have a choice
                 * I can either try again with promises or I can chug along with the accepts
                 * Since I don't know how to even decide what do with previous accepted values I'm just going to chug along with the highest_ballot_num + 1 and accepted_value
                 */
                if self.current_highest_ballot >= highest_ballot.0.unwrap() {
                    if let Some(accepted_value) = &accepted_value {
                        if proposing_value == accepted_value.0 {
                            break;
                        }
                    }
                }

                info!(
                    "Result of promise_qurem {:?},{:?}",
                    highest_ballot, accepted_value
                );
                dbg!(format!(
                    "Result of promise_qurem {:?},{:?}",
                    highest_ballot, accepted_value
                ));
                self.current_highest_ballot = highest_ballot.0.unwrap() + 1; // there will be a highest ballot if there was an error
                if let Some(accepted_value) = accepted_value {
                    proposing_value = accepted_value.0;
                }
            }

            let quarem = ((acceptor_identifiers.len() as f64 / 2.0).floor()) as usize + 1;
            // Now I need to send out accept to everyone
            decided_value = self
                .accept_quarem(
                    proposing_value,
                    acceptor_identifiers,
                    send_to_acceptors,
                    quarem,
                )
                .await;
            info!("Decided value for this loop is {:?}", decided_value);
        }

        assert!(decided_value.is_ok());
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
            proposer_identifer: usize,
        ) -> Result<Option<AcceptedValue>, PromiseReturn> {
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
