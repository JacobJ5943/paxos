use tracing::{info, instrument};

use crate::PromiseReturn;

#[derive(Debug)]
pub struct Acceptor {
    pub promised_ballot_num: Option<usize>,

    // This is just set to 0 as once promised_ballot_num is not None then primmed_node_identifier will have a have
    // This should probably be an option later
    pub promised_node_identifier: usize,
    pub accepted_value: Option<usize>,
}

impl Default for Acceptor {
    fn default() -> Self {
        Self {
            promised_ballot_num: None,
            promised_node_identifier: 0,
            accepted_value: None,
        }
    }
}

impl Acceptor {
    /// .
    ///
    /// I don't know what to do with the errors for this function
    ///
    /// # Errors
    ///
    /// Should error if
    /// 1. Not the right producer (ballot_num and node_identifier don't match fully)
    /// 2. This acceptor has now promised a higher ballot_num (pretty much same as 1)
    /// 3. This acceptor has already accepted a value (This is the case I don't have distinguished yet).
    ///     if producers are following protocol this should have been caught in 2, but we can't tell the difference from that.
    ///     Distinguishing it is only an optimization though as the producer will have to call promise again, in which case if there was a majority value it would learn it.
    ///
    ///
    /// This function will return an error if .
    #[instrument]
    pub fn accept(
        &mut self,
        ballot_num: usize,
        node_identifier: usize,
        value: usize,
    ) -> Result<(), ()> {
        info!("received accept request");
        match self.promised_ballot_num {
            Some(promised_ballot_num) => {
                // Should ballot_num be == or >= ?
                if ballot_num >= promised_ballot_num
                    && node_identifier == self.promised_node_identifier
                {
                    match self.accepted_value {
                        // I might want to just reject Some(_) entirely
                        Some(already_accepted_value) => {
                            if already_accepted_value == value {
                                Ok(())
                            } else {
                                Err(())
                            }
                        }
                        None => {
                            self.accepted_value = Some(value);
                            Ok(())
                        }
                    }
                } else {
                    Err(())
                }
            }
            None => Err(()), // No promise has been received so there shouldn't be an accept
        }
    }

    /// .
    /// Will return Ok(()) if this acceptor has not accepted a value and has not promised a ballot num with a higher value.  node_identifier is used to break ties
    ///
    /// # Panics
    ///
    /// # Errors
    ///
    /// The error contains the PromiseReturn type.  
    /// If this contains a value it's up to the producer to then send out a subsequent accept with this value
    /// If not the producer only needs to update their ballot num and then promise a higher value.
    pub fn promise(
        &mut self,
        ballot_num: usize,
        node_identifier: usize,
    ) -> Result<(), PromiseReturn> {
        info!("received promise request");
        dbg!("We in the promise land");
        if self.promised_ballot_num.is_none() {
            self.promised_ballot_num = Some(ballot_num);
            self.promised_node_identifier = node_identifier;
            return Ok(());
        }

        let promised_ballot_num = self.promised_ballot_num.unwrap();

        if ballot_num >= promised_ballot_num {
            dbg!("that ballot be higher");
            match self.accepted_value {
                Some(accepted_value) => Err(PromiseReturn {
                    highest_ballot_num: promised_ballot_num,
                    accepted_value: self.accepted_value,
                    highest_node_identifier: self.promised_node_identifier,
                }),
                None => {
                    self.promised_ballot_num = dbg!(Some(ballot_num));
                    self.promised_node_identifier = node_identifier;
                    Ok(())
                }
            }
        } else if ballot_num == promised_ballot_num {
            if node_identifier > self.promised_node_identifier {
                match self.accepted_value {
                    Some(accepted_value) => Err(PromiseReturn {
                        highest_ballot_num: promised_ballot_num,
                        accepted_value: self.accepted_value,
                        highest_node_identifier: self.promised_node_identifier,
                    }),
                    None => {
                        self.promised_ballot_num = Some(ballot_num);
                        self.promised_node_identifier = node_identifier;
                        Ok(())
                    }
                }
            } else {
                Err(PromiseReturn {
                    highest_ballot_num: promised_ballot_num,
                    accepted_value: self.accepted_value,
                    highest_node_identifier: self.promised_node_identifier,
                })
            }
        } else {
            Err(PromiseReturn {
                highest_ballot_num: promised_ballot_num,
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
        let mut acceptor = Acceptor::default();
        acceptor.promised_ballot_num = Some(5);
        let result = acceptor.accept(3, 0, 2);
        assert!(result.is_err());
    }

    #[test]
    fn test_reject_non_promised_proposer() {
        let mut acceptor = Acceptor::default();
        acceptor.promised_ballot_num = Some(3);
        acceptor.promised_node_identifier = 0;
        let result = acceptor.accept(3, 1, 2);
        assert!(result.is_err());
    }

    #[test]
    fn no_promise_before_accept() {
        let mut acceptor = Acceptor::default();
        let result = acceptor.accept(1, 0, 1);
        assert!(result.is_err());
    }

    #[test]
    fn correct_accept_after_promise() {
        let mut acceptor = Acceptor::default();
        let promise_result = acceptor.promise(1, 1);
        assert!(promise_result.is_ok());
        let result = acceptor.accept(1, 1, 5);
        assert!(result.is_ok());
    }

    #[test]
    fn higher_ballot_num_correct_proposer() {
        let mut acceptor = Acceptor::default();
        let promise_result = acceptor.promise(1, 1);
        assert!(promise_result.is_ok());
        let result = acceptor.accept(9, 1, 5);
        assert!(result.is_ok());
    }

    #[test]
    fn test_already_accepted_value() {
        let mut acceptor = Acceptor::default();
        acceptor.accepted_value = Some(7);
        acceptor.promised_ballot_num = Some(5);
        acceptor.promised_node_identifier = 1;

        let promise_result = acceptor.promise(8, 1);
        assert!(promise_result.is_err());
        assert_eq!(
            promise_result.unwrap_err(),
            PromiseReturn {
                highest_ballot_num: 5,
                highest_node_identifier: 1,
                accepted_value: Some(7)
            }
        );

        let accept_response = acceptor.accept(8, 1, 9);
        assert!(accept_response.is_err());
    }
}
