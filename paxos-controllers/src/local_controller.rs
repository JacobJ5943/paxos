use std::{collections::HashMap, sync::Arc};

use basic_paxos_lib::{
    acceptors::{AcceptedValue, Acceptor},
    HighestBallotPromised, HighestSlotPromised, PromiseReturn, SendToAcceptors,
};
use tokio::sync::{
    mpsc::{error::SendError, UnboundedSender},
    Mutex,
};

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum Messages {
    AcceptRequest {
        acceptor_id: usize,
        proposer_id: usize,
        slot_num: usize,
        ballot_num: usize,
        value: usize,
    },
    AcceptResponse {
        acceptor_id: usize,
        proposer_id: usize,
        accept_result: Result<AcceptedValue, (HighestSlotPromised, HighestBallotPromised)>,
    },
    PromiseRequest {
        acceptor_id: usize,
        slot_num: usize,
        ballot_num: usize,
        proposer_id: usize,
    },
    PromiseResponse {
        acceptor_id: usize,
        proposer_id: usize,
        promise_result: Result<Option<AcceptedValue>, PromiseReturn>,
    },
}

#[derive(Default)]
pub struct LocalMessageController {
    pub acceptors: HashMap<usize, Arc<Mutex<Acceptor>>>, // <AcceptorIdentifier, Acceptor>
    pub current_messages: Arc<Mutex<Vec<(Messages, UnboundedSender<Messages>)>>>,
}

#[derive(Debug)]
pub enum ControllerErrors {
    MessageNotFound,
    SendError(SendError<Messages>),
}

impl From<SendError<Messages>> for ControllerErrors {
    fn from(val: SendError<Messages>) -> Self {
        ControllerErrors::SendError(val)
    }
}

impl LocalMessageController {
    pub fn new(acceptors: HashMap<usize, Arc<Mutex<Acceptor>>>) -> (Self, LocalMessageSender) {
        let current_messages = Arc::new(Mutex::new(Vec::new()));
        let current_messages_cloned = current_messages.clone();
        let (sender, receiver) =
            tokio::sync::mpsc::unbounded_channel::<(Messages, UnboundedSender<Messages>)>();

        tokio::spawn(add_messages_to_queue(current_messages_cloned, receiver));

        (
            Self {
                acceptors,
                current_messages,
            },
            LocalMessageSender {
                sender_to_controller: sender,
            },
        )
    }


    /// If there are multiple messages that are Eq then the last message in the waiting list (Most recently sent) will be sent.
    /// This is becuase I'm not sure what the return type of this function would be other wise.
    /// 
    /// The only cases of multiple messages being Eq in the queue would be PromiseReturn and AcceptReturn
    /// In both cases I'm okay with this because only the latest message will have anybody listening to it.
    /// This is becuase my current poc has the proposer locked behind a Mutex
    /// 
    /// This would need to be changed if a proposer was ever not behind a Mutex.  Though I'm not sure how the proposer would work if not behind a mutex
    pub async fn try_send_message(
        &mut self,
        message_to_send: &Messages,
    ) -> Result<Messages, ControllerErrors> {
        println!("trying to remove message {:?}", &message_to_send);
        let mut x = self
            .current_messages
            .lock()
            .await
            .iter()
            .enumerate()
            .filter(|(_, (message_in_vec, _))| message_in_vec == message_to_send)
            .map(|(index, _other)| index)
            .clone()
            .collect::<Vec<usize>>();

        
        if x.is_empty() {
            Err(ControllerErrors::MessageNotFound)
        } else {
            let last_index = x.pop().unwrap();
            let (message, sender) = self.current_messages.lock().await.remove(last_index);

            // Now clear out the previous messages
            for x in x.into_iter().rev() {
                self.current_messages.lock().await.remove(x);
            }

            match &message {
                Messages::AcceptRequest {
                    acceptor_id,
                    proposer_id,
                    slot_num,
                    ballot_num,
                    value,
                } => {
                    // The lock is only for the duration of the accept which is not waiting on other messages so a deadlock cannot occur
                    let response = self
                        .acceptors
                        .get_mut(acceptor_id)
                        .unwrap()
                        .lock()
                        .await
                        .accept(*ballot_num, *slot_num, *proposer_id, *value);
                    let accept_response = Messages::AcceptResponse {
                        acceptor_id: *acceptor_id,
                        proposer_id: *proposer_id,
                        accept_result: response,
                    };
                    self.current_messages
                        .lock()
                        .await
                        .push((accept_response.clone(), sender));
                    Ok(accept_response)
                }
                Messages::AcceptResponse {
                    acceptor_id:_,
                    proposer_id:_,
                    accept_result:_,
                } => match sender.send(message.clone()) {
                    Ok(()) => Ok(message),
                    Err(send_error) => Err(ControllerErrors::from(send_error)),
                },
                Messages::PromiseRequest {
                    acceptor_id,
                    proposer_id,
                    slot_num,
                    ballot_num,
                } => {
                    // The lock is only for the duration of the promise which is not waiting on other messages so a deadlock cannot occur
                    let response = self
                        .acceptors
                        .get_mut(acceptor_id)
                        .unwrap()
                        .lock()
                        .await
                        .promise(*ballot_num, *slot_num, *proposer_id);
                    let promise_response = Messages::PromiseResponse {
                        acceptor_id: *acceptor_id,
                        proposer_id: *proposer_id,
                        promise_result: response,
                    };
                    self.current_messages
                        .lock()
                        .await
                        .push((promise_response.clone(), sender));
                    Ok(promise_response)
                }
                Messages::PromiseResponse { .. } => {
                    let send_result = sender.send(message.clone());
                    match send_result {
                        Ok(()) => Ok(message),
                        Err(send_error) => Err(ControllerErrors::from(send_error)),
                    }
                }
            }
        }
    }
}

// This should really be a bounded sender since it's just where to send the response
async fn add_messages_to_queue(
    current_messages: Arc<Mutex<Vec<(Messages, UnboundedSender<Messages>)>>>,
    mut receiver: tokio::sync::mpsc::UnboundedReceiver<(Messages, UnboundedSender<Messages>)>,
) {
    loop {
        println!("waiting for that sweet sweet message");
        let incoming_message = receiver.recv().await.unwrap();
        println!("We got a message {:?}", &incoming_message);
        current_messages.lock().await.push(incoming_message);
    }
}

#[derive(Debug, Clone)]
pub struct LocalMessageSender {
    pub sender_to_controller: UnboundedSender<(Messages, UnboundedSender<Messages>)>, // The sender is so that the response can also be controlled
}

#[async_trait::async_trait]
impl SendToAcceptors for LocalMessageSender {
    async fn send_accept(
        &self,
        acceptor_identifier: usize,
        value: usize,
        slot_num: usize,
        ballot_num: usize,
        proposer_identifier: usize,
    ) -> Result<AcceptedValue, (HighestSlotPromised, HighestBallotPromised)> {
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        let send_result = self.sender_to_controller.send((
            Messages::AcceptRequest {
                acceptor_id: acceptor_identifier,
                proposer_id: proposer_identifier,
                slot_num,
                ballot_num,
                value,
            },
            sender,
        ));
        assert!(send_result.is_ok());
        let result = receiver.recv().await.unwrap();

        match result {
            Messages::AcceptResponse {
                acceptor_id: _acceptor_identifier,
                proposer_id: _proposer_identifier,
                accept_result,
            } => accept_result,
            _ => panic!("Incorrect response from accept request"),
        }
    }

    async fn send_promise(
        &self,
        acceptor_identifier: usize,
        slot_num: usize,
        ballot_num: usize,
        proposer_identifier: usize,
    ) -> Result<Option<AcceptedValue>, PromiseReturn> {
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        let send_result = self.sender_to_controller.send((
            Messages::PromiseRequest {
                acceptor_id: acceptor_identifier,
                proposer_id: proposer_identifier,
                slot_num,
                ballot_num,
            },
            sender,
        ));
        assert!(send_result.is_ok());
        let result = receiver.recv().await.unwrap();

        match result {
            Messages::PromiseResponse { promise_result, .. } => promise_result,
            _ => panic!("Incorrect response from promise request"),
        }
    }
}
