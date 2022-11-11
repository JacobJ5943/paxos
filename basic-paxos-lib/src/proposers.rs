use std::collections::HashMap;

use futures::StreamExt;
use tracing::{info, instrument};

use crate::{
    acceptors::AcceptedValue, HighestBallotPromised, HighestSlotPromised, PromiseReturn,
    SendToAcceptors,
};

#[derive(Debug, Default)]
pub struct Proposer {
    pub current_highest_ballot: usize,
    pub node_identifier: usize,
    pub highest_slot: usize,
}

#[derive(Debug, Eq, PartialEq)]
pub enum ProposingErrors {
    NewSlot(HighestSlotPromised, HighestBallotPromised),
    NetworkError, // TODO actually use this error
}

impl Proposer {
    pub fn new(node_identifier: usize) -> Self {
        Self {
            current_highest_ballot: 0,
            node_identifier,
            highest_slot: 0,
        }
    }

    // Tests moved to basic-paxos-lib-tests

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
        proposing_slot: usize,
        send_to_acceptors: &impl SendToAcceptors,
    ) -> Result<
        (),
        (
            HighestBallotPromised,
            HighestSlotPromised,
            Option<AcceptedValue>,
        ),
    > {
        let mut sending_promises = Vec::new();

        self.current_highest_ballot += 1;
        for acceptor_id in acceptor_identifiers.iter() {
            sending_promises.push(send_to_acceptors.send_promise(
                *acceptor_id,
                self.highest_slot,
                self.current_highest_ballot,
                self.node_identifier,
            ));
        }

        let mut promise_futures_unsorted =
            futures::stream::FuturesUnordered::from_iter(sending_promises.into_iter());

        // Get the first quorum results

        let quorum = ((acceptor_identifiers.len() as f64 / 2.0).floor()) as usize + 1;

        let mut results = Vec::new();
        while results.len() < quorum && !promise_futures_unsorted.is_empty() {
            let new_result = dbg!(promise_futures_unsorted.select_next_some().await);
            results.push(new_result);
        }

        let mut highest_ballot_for_value: Option<usize> = None;

        let mut working_promise: Option<PromiseReturn> = None;
        for result in results.into_iter() {
            //dbg!(&result);
            match (result, &mut working_promise) {
                (Ok(accepted_value), None) => {
                    if let Some(accepted_value) = accepted_value {
                        working_promise = Some(PromiseReturn {
                            highest_ballot_num: self.current_highest_ballot,
                            current_slot_num: proposing_slot,
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
                    HighestSlotPromised(working_promise_err.current_slot_num),
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
                //AwaitingAccept {
                //acceptor_id: *acceptor_id,
                //}
                send_to_acceptors.send_accept(
                    *acceptor_id,
                    proposing_value,
                    self.highest_slot,
                    self.current_highest_ballot,
                    self.node_identifier,
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
                    match accepted_results.entry(value_accepted.0) {
                        std::collections::hash_map::Entry::Occupied(oc) => {
                            *oc.into_mut() += 1;
                        }
                        std::collections::hash_map::Entry::Vacant(vac) => {
                            vac.insert(1);
                        }
                    }
                    if accepted_results.get(&value_accepted.0).unwrap() >= &quorum {
                        //return Ok(value_accepted);// figure out short circuiting later
                        // There was a test that requires all 3/3 acceptors to have accepted.  Only 2/3 did with the short circuit so while decided still failed
                        decided_value = Some(value_accepted)
                        // This value has been accepted
                    }
                }
                Err(_highest_ballot_promised) => {
                    // todo
                    // Should this have a todo!() macro?
                    // This would be if there was a competing proposer with the same ballot number
                }
            }
        }

        if let Some(decided_value) = decided_value {
            Ok(decided_value.0)
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
        proposing_slot: usize, // I want the propose_value to take a slot since the retry logic if the slot is taken should be up to the user.  This could be changed with a try_propose_value.   Or use like a propose_value_weak as in compare_exchange_weak, compare_exchange
        send_to_acceptors: &impl SendToAcceptors,
        acceptor_identifiers: &mut Vec<usize>,
        _total_acceptor_count: usize, // unused for now, but will probably be a config thing to allow for resizing.  This would determine the quorum based on the slot I suppose?
    ) -> Result<usize, ProposingErrors> {
        // TODO This return type will need to be changed.  This error needs to be either a continuation error where the next slot number is given with the highest promised ballot num at that slot. // The other error type could be some kind of network error
        info!("Proposing_value");

        // This is mut for the case where an acceptor has already accepted a value
        let mut proposing_value = initial_proposed_value;

        let mut decided_value = Err(());

        while decided_value.is_err() {
            info!("Starting promise while loop.");
            while let Err((highest_ballot, highest_slot_proposed, accepted_value)) = dbg!(
                self.promise_quorum(acceptor_identifiers, proposing_slot, send_to_acceptors)
                    .await
            ) {
                if *highest_slot_proposed > proposing_slot {
                    return Err(ProposingErrors::NewSlot(
                        highest_slot_proposed,
                        highest_ballot,
                    ));
                }
                assert!(highest_slot_proposed.0 == proposing_slot, "The acceptor should have promised this slot then in which case there wouldn't have been Err");
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
            Ok(decided_value) => Ok(decided_value),
            Err(_) => {
                // This is the case because if a value is not decided the while let loop above will not exit
                // This could also be if I broke early from the while loop, but I think if that's the case I would want to return the error in that instant?
                unreachable!()
            }
        }
    }
}

#[cfg(test)]
mod prop_tests {
    // I'm not sure how to test any of these here so instead they will be tested in the basic-paxos-lib-tests crate
}
