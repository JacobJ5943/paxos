use tracing::{instrument, info};

use crate::PromiseReturn;

#[derive(Debug)]
pub struct Acceptor {
    promised_ballot_num:Option<usize>,

    // This is just set to 0 as once promised_ballot_num is not None then primosed_node_identifier will have a have
    // This should probably be an option later
    promised_node_identifier:usize, 
    accepted_value:Option<usize>
}

impl Default for Acceptor {
    fn default() -> Self {
        Self { promised_ballot_num: None, promised_node_identifier: 0, accepted_value: None }
    }
}


        
impl Acceptor {
    /// .
    /// 
    /// I don't know what to do wtih the errors for this function
    /// 
    /// # Errors
    /// 
    /// Should error if
    /// 1. Not the right producer (ballot_num and node_identifier don't match fully)
    /// 2. This acceptor has now promised a higher ballot_num (pretty much same as 1)
    /// 3. This acceptor has already accepted a value (This is the case I don't have distiguised yet).
    ///     if producers are following protocol this should have been caught in 2, but we can't tell the difference from that.
    ///     Distinguishing it is only an optimization though as the producer will have to call promise again, in which case if there was a majority value it would learn it.
    /// 
    /// 
    /// This function will return an error if .
    #[instrument]
    pub fn accept(&mut self, ballot_num:usize, node_identifier:usize, value:usize) -> Result<(),()>{
        info!("recieved accept request");
        match self.promised_ballot_num {
            Some(promised_ballot_num) => {
                if ballot_num == promised_ballot_num && node_identifier == self.promised_node_identifier{
                    self.accepted_value = Some(value);
                    Ok(())
                } else {
                    match self.accepted_value {
                        Some(accepted_value) => if accepted_value == value {
                            Ok(())
                        } else {
                            Err(())
                        },
                        None => Err(()),
                    }
                }
            },
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
    /// If this contains a value it's up to the producer to then send out a subsiquent accept with this value
    /// If not the producer only needs to update their ballot num and then promise a higher value.
    pub fn promise(&mut self, ballot_num:usize, node_identifier:usize) -> Result<(),PromiseReturn> {
        info!("received promise request");
        if self.promised_ballot_num.is_none() {
            self.promised_ballot_num = Some(ballot_num);
            self.promised_node_identifier = node_identifier;
            return Ok(())
        } 

        let promised_ballot_num = self.promised_ballot_num.unwrap();

        if ballot_num >= promised_ballot_num {
            match self.accepted_value {
                Some(accepted_value) => {
                    Err(PromiseReturn {
                        highest_ballot_num:promised_ballot_num,
                        accepted_value: self.accepted_value,
                        highest_node_identifier: self.promised_node_identifier,
                    })
                }
                None => {
                    self.promised_ballot_num = Some(ballot_num);
                    self.promised_node_identifier = node_identifier;
                    Ok(())
                },
            }
        } else if ballot_num == promised_ballot_num {
            if node_identifier > self.promised_node_identifier {
                match self.accepted_value {
                Some(accepted_value) => {
                    Err(PromiseReturn {
                        highest_ballot_num:promised_ballot_num,
                        accepted_value: self.accepted_value,
                        highest_node_identifier: self.promised_node_identifier,
                    })
                }
                None => {
                    self.promised_ballot_num = Some(ballot_num);
                    self.promised_node_identifier = node_identifier;
                    Ok(())
                },
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


