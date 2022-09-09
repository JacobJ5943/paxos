use tracing::{info, instrument};

use crate::{acceptors::Acceptor, PromiseReturn};

#[derive(Debug)]
struct Proposer {
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
    #[instrument]
    fn propose_value(
        &mut self,
        initial_proposed_value: usize,
        mut acceptors: &mut Vec<Acceptor>,
        total_acceptor_count: usize,
    ) -> Result<(), usize> {
        info!("Proposing_value");
        // This is mut for the case where an acceptor has already accepted a value
        let mut proposing_value = initial_proposed_value;

        let workign_promise_response: Option<PromiseReturn> = None;
        let acceptor_response_iter = acceptors
            .iter_mut()
            // Send promise to all acceptors
            .map(|acceptor| {
                dbg!(acceptor.promise(self.current_highest_ballot + 1, self.node_identifier))
            });

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
        for (acceptor_index, response) in acceptor_response_iter.enumerate() {
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
    use crate::{acceptors::Acceptor, proposers::Proposer};

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

    #[test]
    fn test_only_one_proposer() {
        let mut acceptors = vec![
            Acceptor::default(),
            Acceptor::default(),
            Acceptor::default(),
        ];

        let mut proposer = Proposer {
            current_highest_ballot: 0,
            node_identifier: 0,
        };

        let result = proposer.propose_value(5, &mut acceptors, 5);

        assert!(result.is_ok());
        // Currently this is manually being checked by looking at the tracing output
        // This should be automated by tracing_test, but first get some tests out and debug
        //todo!();
    }

    #[test]
    fn test_proposer_a_sends_2_to_two_then_proposer_b_sends_3_to_all_3() {
        let mut acceptors = vec![Acceptor::default(), Acceptor::default()];

        let mut proposer_a = Proposer {
            current_highest_ballot: 0,
            node_identifier: 0,
        };

        let mut proposer_b = Proposer {
            current_highest_ballot: 0,
            node_identifier: 1,
        };

        let prop_a_result = proposer_a.propose_value(2, &mut acceptors, 3);
        acceptors.push(Acceptor::default());
        let prop_b_result = proposer_b.propose_value(5, &mut acceptors, 3);

        assert!(prop_a_result.is_ok());
        assert!(prop_b_result.is_err());
        assert_eq!(prop_b_result.unwrap_err(), 2);
    }

    // I'm not totally sure what I want form this test
    // Currently if the acceptors are promised to a proposer.  If a proposer sends a higher ballot number they will accept it.
    // This means after the proposer has sent out all of the promises it can just send an accept with the higher number
    // so the acceptors should accept the value, but might not change their highest ballot count.
    #[test]
    fn test_take_highest_value_ballot_count() {
        // I should probably set the node identifier so I don't rely on the default?
        let mut acceptors = vec![
            Acceptor::default(),
            Acceptor::default(),
            Acceptor::default(),
        ];
        acceptors[0].promised_ballot_num = Some(5);
        acceptors[1].promised_ballot_num = Some(7);
        acceptors[2].promised_ballot_num = Some(4);

        let mut proposer_a = Proposer {
            current_highest_ballot: 0,
            node_identifier: 0,
        };

        let prop_a_result = proposer_a.propose_value(2, &mut acceptors, 3);
        assert_eq!(proposer_a.current_highest_ballot, 7);

        assert_eq!(acceptors[0].accepted_value, Some(2));
        assert_eq!(acceptors[1].accepted_value, Some(2));
        assert_eq!(acceptors[2].accepted_value, Some(2));
        //assert_eq!(acceptors[0].promised_ballot_num,Some(8));
        //assert_eq!(acceptors[1].promised_ballot_num,Some(8));
        //assert_eq!(acceptors[2].promised_ballot_num,Some(8));
    }
}
