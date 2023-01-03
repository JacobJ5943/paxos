use std::ops::Deref;

use acceptors::AcceptedValue;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

pub mod acceptors;

pub mod proposers;

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize, Clone)] // These should be behind a feature flag probably
pub struct PromiseReturn {
    highest_ballot_num: usize,
    current_slot_num: usize,
    highest_node_identifier: usize,
    accepted_value: Option<usize>,
}

#[async_trait]
/// The trait that is used to control how messages are sent to acceptors.
///
/// This is useful to have for supporting both fully local as well as network communication
pub trait SendToAcceptors {
    async fn send_accept(
        &self,
        acceptor_identifier: usize,
        value: usize,
        slot_num: usize,
        ballot_num: usize,
        proposer_identifier: usize,
    ) -> Result<AcceptedValue, (HighestSlotPromised, HighestBallotPromised)>;
    async fn send_promise(
        &self,
        acceptor_identifier: usize,
        slot_num: usize,
        ballot_num: usize,
        proposer_identifier: usize,
    ) -> Result<Option<AcceptedValue>, PromiseReturn>;
}

#[derive(Debug, Deserialize, Serialize, Clone, Eq, PartialEq)]
pub struct HighestSlotPromised(pub usize);

impl Deref for HighestSlotPromised {
    type Target = usize;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
#[derive(Debug, Deserialize, Serialize, Clone, Eq, PartialEq)]
pub struct HighestBallotPromised(pub Option<usize>);
