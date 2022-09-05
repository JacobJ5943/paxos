use tracing::{instrument, info};

use crate::{acceptors::Acceptor, PromiseReturn};

#[derive(Debug)]
struct Proposer {
    current_highest_ballot: usize,
    node_identifier: usize,
}

impl Proposer {
    /// .
    /// TODO acceptors should not be this.
    /// # Errors
    ///
    /// This function will return an error if there is already an accepted value.  The value of the error will be that accepted value.
    #[instrument]
    fn propose_value(
        &mut self,
        initial_proposed_value: usize,
        mut acceptors: Vec<Acceptor>,
        total_acceptor_count: usize,
    ) -> Result<(), usize> {
        info!("Proposing_value");
        // This is mut for the case where an acceptor has already accepted a value
        let mut proposing_value = initial_proposed_value;

        let working_promise_response =  acceptors
            .iter_mut()
            // Send promise to all acceptors
            .map(|acceptor| acceptor.promise(self.current_highest_ballot + 1, self.node_identifier))
            .filter(Result::is_err)
            .map(Result::unwrap_err)
            .filter(|r|r.accepted_value.is_some())
            .reduce(|previous_response, curr_response| {
                // I'm keeping this as is while I figure out if I have it correct
                if curr_response.highest_ballot_num > previous_response.highest_ballot_num {
                    curr_response
                } else if curr_response.highest_ballot_num == previous_response.highest_ballot_num{
                    if curr_response.highest_node_identifier > previous_response.highest_node_identifier {
                        curr_response
                    } else {
                        previous_response
                    }
                } else {
                    previous_response
                }
            });
        

        if working_promise_response.is_some() {
            let working_promise_response = working_promise_response.unwrap();
            self.current_highest_ballot = working_promise_response.highest_ballot_num;

            if !working_promise_response.accepted_value.is_none() {
                proposing_value = working_promise_response.accepted_value.unwrap();
            }

            // It might need to send prepare requests again?  I'm not totally sure
            // as long as I use the a higher ballot num than all responses I think I'm okay to try for accepts.
            // If I don't get a majority of accepts than I should retry with the promises
        }

        let accepted_count = acceptors
            .iter_mut()
            .map(|acceptor| {
                acceptor.accept(
                    self.current_highest_ballot + 1,
                    self.node_identifier,
                    proposing_value,
                )
            })
            .filter(Result::is_ok)
            .count();

        if accepted_count >= (total_acceptor_count / 2 ) + 1 {
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
    use crate::{acceptors::Acceptor, proposers::Proposer};

    // IMPORTANT TODO look into tracing-test crate

    #[test]
    fn testing_usize_division(){
        assert_eq!(3/2,1);
        assert_eq!((3/2) + 1,2);
        assert_eq!((4/2) + 1,3);
    }
    #[test]
    fn it_works() {
        let result = 2 + 2;
        assert_eq!(result, 4);
    }





    #[test]
    fn test_only_one_proposer(){
        let my_sub = tracing_subscriber::FmtSubscriber::new();
        tracing::subscriber::set_global_default(my_sub).expect("This should not fail");
        let mut acceptors = vec![Acceptor::default(),Acceptor::default(),Acceptor::default()];

        let mut proposer = Proposer {
            current_highest_ballot: 0,
            node_identifier: 0,
        };

        let result = proposer.propose_value(5, acceptors, 5);

        assert!(result.is_ok());
        // Currently this is manually being checked by looking at the tracing output
        // This should be automated by tracing_test, but first get some tests out and debug
        //todo!();
    }
}