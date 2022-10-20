use acceptors::{AcceptedValue, HighestBallotPromised};
use async_trait::async_trait;
use serde::{Serialize, Deserialize};

pub mod acceptors;

pub mod proposers;


#[derive(Debug, PartialEq, Eq)]
#[derive(Serialize, Deserialize)] // These should be behind a featuer flag probably
pub struct PromiseReturn {
    highest_ballot_num:usize,
    highest_node_identifier:usize,
    accepted_value:Option<usize>
}

#[async_trait]
/// DO I want this to send to all acceptors or just one?
/// 
pub trait SendToAcceptors {

    async fn send_accept(&self, acceptor_identifier:usize, value:usize, ballot_num:usize, proposer_identifier:usize) -> Result<AcceptedValue, HighestBallotPromised>;
    async fn send_promise(&self, acceptor_identifier:usize, ballot_num:usize, proposer_identifier:usize)-> Result<(), PromiseReturn>;
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
}
