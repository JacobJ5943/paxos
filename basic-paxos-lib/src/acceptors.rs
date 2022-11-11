use serde::{Deserialize, Serialize};
use tracing::{info, instrument};

use crate::{HighestBallotPromised, HighestSlotPromised, PromiseReturn};

#[derive(Debug, Default, Clone)]
pub struct Acceptor {
    pub promised_ballot_num: Option<usize>,
    pub promised_slot_num: usize,

    // This is just set to 0 as once promised_ballot_num is not None then primmed_node_identifier will have a have
    // This should probably be an option later
    pub promised_node_identifier: usize,
    pub accepted_value: Option<usize>,
}

#[derive(Debug, Deserialize, Serialize, Clone, Eq, PartialEq)]
pub struct AcceptedValue(pub usize);

impl Acceptor {
    /// This is how an acceptor receives accept requests.
    ///
    /// Returns Ok of the value Accepted by this acceptor if the ballot number is greater than or equal to what the acceptor has promised.
    ///
    /// If the Acceptor had not accepted a value then if successful the value returned will match the value of the request.
    ///
    /// # Errors
    ///
    /// Should error if
    /// 1. the self.promised_ballot_num is > ballot_num
    /// 2. if self.promised_ballot_num is >= ballot_num but the node_identifier is less than the promised node identifier
    ///
    #[instrument]
    pub fn accept(
        &mut self,
        ballot_num: usize,
        slot_num: usize,
        node_identifier: usize,
        value: usize,
    ) -> Result<AcceptedValue, (HighestSlotPromised, HighestBallotPromised)> {
        info!("received accept request");
        if let Some(accepted_value) = self.accepted_value {
            return Ok(AcceptedValue(accepted_value));
        };
        if slot_num != self.promised_slot_num {
            return Err((
                HighestSlotPromised(self.promised_slot_num),
                HighestBallotPromised(self.promised_ballot_num),
            ));
        }

        match self.promised_ballot_num {
            Some(promised_ballot_num) => {
                // Should ballot_num be == or >= ?
                if (ballot_num > promised_ballot_num)
                    || (promised_ballot_num == ballot_num
                        && self.promised_node_identifier == node_identifier)
                {
                    match self.accepted_value {
                        // I might want to just reject Some(_) entirely
                        Some(already_accepted_value) => Ok(AcceptedValue(already_accepted_value)),
                        None => {
                            self.accepted_value = Some(value);
                            Ok(AcceptedValue(value))
                        }
                    }
                } else {
                    Err((
                        HighestSlotPromised(self.promised_slot_num),
                        HighestBallotPromised(Some(promised_ballot_num)),
                    ))
                }
            }
            None => Err((
                HighestSlotPromised(self.promised_slot_num),
                HighestBallotPromised(None),
            )), // No promise has been received so there shouldn't be an accept
        }
    }

    /// This is how an acceptor receives promise requests.
    /// Will return Ok if the ballot_num is greater than the ballot_num already promised by this acceptor.
    /// If they are the same the node identifier breaks the tie
    ///
    /// This Ok response will contain the accepted value by this acceptor if it has already accepted a value.
    ///
    ///
    /// # Errors
    ///
    /// This function will return an error if the ballot_num is less than the ballot num already promised by this acceptor.
    pub fn promise(
        &mut self,
        ballot_num: usize,
        slot_num: usize,
        node_identifier: usize,
    ) -> Result<Option<AcceptedValue>, PromiseReturn> {
        info!("received promise request");
        dbg!("We in the promise land");

        if slot_num > self.promised_slot_num {
            self.promised_slot_num = slot_num;
            self.promised_ballot_num = Some(ballot_num);
            self.promised_node_identifier = node_identifier;
            self.accepted_value = None; // Since this is a new slot we haven't accepted for it yet
            return Ok(None);
        }

        if slot_num < self.promised_slot_num {
            return Err(PromiseReturn {
                highest_ballot_num: self
                    .promised_ballot_num
                    .expect("ballot nums are always increasing and never reset to 0"), // I think later I will make thtis a ball o t number per slot maybe?
                current_slot_num: self.promised_slot_num,
                highest_node_identifier: self.promised_node_identifier,
                accepted_value: self.accepted_value,
            });
        }

        if self.promised_ballot_num.is_none() {
            self.promised_ballot_num = Some(ballot_num);
            self.promised_node_identifier = node_identifier;
            return Ok(self.accepted_value.map(AcceptedValue));
        }

        let promised_ballot_num = self.promised_ballot_num.unwrap();

        if ballot_num >= promised_ballot_num {
            dbg!("that ballot be higher");
            match self.accepted_value {
                Some(_accepted_value) => Ok(self.accepted_value.map(AcceptedValue)), // Do I need to update my promised ballot num?  This would be for a correctness reason if so
                None => {
                    self.promised_ballot_num = dbg!(Some(ballot_num));
                    self.promised_node_identifier = node_identifier;
                    Ok(self.accepted_value.map(AcceptedValue))
                }
            }
        } else if ballot_num == promised_ballot_num {
            if node_identifier > self.promised_node_identifier {
                match self.accepted_value {
                    Some(_accepted_value) => Ok(self.accepted_value.map(AcceptedValue)), // Do I need to update my promised ballot num?  This would be for a correctness reason if so
                    None => {
                        self.promised_ballot_num = Some(ballot_num);
                        self.promised_node_identifier = node_identifier;
                        Ok(self.accepted_value.map(AcceptedValue))
                    }
                }
            } else {
                Err(PromiseReturn {
                    highest_ballot_num: promised_ballot_num,
                    current_slot_num: self.promised_slot_num,
                    accepted_value: self.accepted_value,
                    highest_node_identifier: self.promised_node_identifier,
                })
            }
        } else {
            Err(PromiseReturn {
                highest_ballot_num: promised_ballot_num,
                current_slot_num: self.promised_slot_num,
                accepted_value: self.accepted_value,
                highest_node_identifier: self.promised_node_identifier,
            })
        }
    }
}

#[cfg(test)]
mod acc_tests {

    use crate::{acceptors::Acceptor, PromiseReturn};

    #[test]
    fn test_reject_lower_ballot_num() {
        let mut acceptor = Acceptor {
            promised_ballot_num: Some(5),
            ..Default::default()
        };
        let result = acceptor.accept(3, 0, 0, 2);
        assert!(result.is_err());
    }

    #[test]
    fn test_reject_non_promised_proposer() {
        let mut acceptor = Acceptor {
            promised_ballot_num: Some(3),
            promised_node_identifier: 0,
            ..Default::default()
        };
        let result = acceptor.accept(3, 0, 1, 2);
        assert!(result.is_err());
    }

    #[test]
    fn no_promise_before_accept() {
        let mut acceptor = Acceptor::default();
        let result = acceptor.accept(1, 0, 0, 1);
        assert!(result.is_err());
    }

    #[test]
    fn correct_accept_after_promise() {
        let mut acceptor = Acceptor::default();
        let promise_result = acceptor.promise(1, 0, 1);
        assert!(promise_result.is_ok());
        let result = acceptor.accept(1, 0, 1, 5);
        assert!(result.is_ok());
    }

    #[test]
    fn higher_ballot_num_correct_proposer() {
        let mut acceptor = Acceptor::default();
        let promise_result = acceptor.promise(1, 0, 1);
        assert!(promise_result.is_ok());
        let result = acceptor.accept(9, 0, 1, 5);
        assert!(result.is_ok());
    }

    #[test]
    fn promise_request_with_higher_slot_lower_ballot_num() {
        let mut acceptor = Acceptor {
            promised_ballot_num: Some(5),
            promised_slot_num: 1,
            promised_node_identifier: 1,
            accepted_value: Some(3),
        };

        let result = acceptor.promise(2, 2, 1);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), None);
    }

    #[test]
    fn promise_request_with_higher_slot_higher_ballot() {
        let mut acceptor = Acceptor {
            promised_ballot_num: Some(5),
            promised_slot_num: 1,
            promised_node_identifier: 1,
            accepted_value: Some(3),
        };

        let result = acceptor.promise(7, 2, 1);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), None);
    }

    #[test]
    fn promise_request_with_lower_slot_higher_ballot() {
        let mut acceptor = Acceptor {
            promised_ballot_num: Some(5),
            promised_slot_num: 2,
            promised_node_identifier: 1,
            accepted_value: Some(3),
        };

        let result = acceptor.promise(7, 1, 1);
        assert!(result.is_err());

        let expected_promise_return = PromiseReturn {
            highest_ballot_num: 5,
            current_slot_num: 2,
            highest_node_identifier: 1,
            accepted_value: Some(3),
        };
        assert_eq!(result.unwrap_err(), expected_promise_return);
    }

    #[test]
    fn promise_request_with_lower_slot_lower_ballot() {
        let mut acceptor = Acceptor {
            promised_ballot_num: Some(5),
            promised_slot_num: 2,
            promised_node_identifier: 1,
            accepted_value: Some(3),
        };

        let result = acceptor.promise(7, 1, 1);
        assert!(result.is_err());

        let expected_promise_return = PromiseReturn {
            highest_ballot_num: 5,
            current_slot_num: 2,
            highest_node_identifier: 1,
            accepted_value: Some(3),
        };
        assert_eq!(result.unwrap_err(), expected_promise_return);
    }

    #[test]
    fn promise_request_with_lower_slot() {
        let mut acceptor = Acceptor {
            promised_ballot_num: Some(1),
            promised_slot_num: 1,
            promised_node_identifier: 1,
            accepted_value: None,
        };

        let promise_result = acceptor.promise(2, 0, 1);
        assert!(promise_result.is_err());

        let actual_promise_return = promise_result.unwrap_err();
        let expected_promise_return = PromiseReturn {
            highest_ballot_num: 1,
            current_slot_num: 1,
            highest_node_identifier: 1,
            accepted_value: None,
        };
        assert_eq!(actual_promise_return, expected_promise_return);
    }

    #[test]
    fn test_already_accepted_value_for_slot() {
        let mut acceptor = Acceptor {
            promised_ballot_num: Some(5),
            promised_slot_num: 0,
            promised_node_identifier: 1,
            accepted_value: Some(7),
        };

        let promise_result = acceptor.promise(2, 0, 1);
        assert!(promise_result.is_err());
        assert_eq!(
            promise_result.unwrap_err(),
            PromiseReturn {
                highest_ballot_num: 5,
                current_slot_num: 0,
                highest_node_identifier: 1,
                accepted_value: Some(7)
            }
        );

        let accept_response = acceptor.accept(8, 0, 1, 9);
        assert!(accept_response.is_ok());
        assert!(accept_response.unwrap().0 != 9)
    }
}
