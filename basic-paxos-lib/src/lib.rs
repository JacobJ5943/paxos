use acceptors::{AcceptedValue, HighestBallotPromised};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

pub mod acceptors;

pub mod proposers;

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)] // These should be behind a featuer flag probably
pub struct PromiseReturn {
    highest_ballot_num: usize,
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
        ballot_num: usize,
        proposer_identifier: usize,
    ) -> Result<AcceptedValue, HighestBallotPromised>;
    async fn send_promise(
        &self,
        acceptor_identifier: usize,
        ballot_num: usize,
        proposer_identifier: usize,
    ) -> Result<Option<AcceptedValue>, PromiseReturn>;
}

// These will contain the integration tests
// The hard part about these tests is that I don't currently have a way to
// run different parts at different times.  I can only run a proposer's full path
//
// I would like to split it up for testing, but I don't think that's necessarily required
// Maybe when I do the server I can man in the middle it and control when messages send
#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        let result = 2 + 2;
        assert_eq!(result, 4);
    }

    #[test]
    // TODO how do I mock this here?
    fn test_that_one_deadlock_situation_where_A_proposes_and_accepts_at_one_then_b_proposes_and_accepts_then_a_is_stuck_proposing(
    ) {
        todo!();
    }
}
