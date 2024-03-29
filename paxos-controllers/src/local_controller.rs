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

/// The controller used to decide when messages are sent and received
///
/// This is useful when creating demos.
///
///
/// It's design is to have a tokio::task listening for messages and then adding them to current_messages
// Why don't I save off the handle for this task and add it to Drop?
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

    ///
    /// Sends the [Messages][`Messages`] Eq to message_to_send.  If multiple [Messages][`Messages`] are Eq then all are removed from the queue with the Message more recently added sent.
    ///
    /// If there is no [message][`Messages`] Eq to message_to_send then Err
    /// If there were other errors sending the [Messages][`Messages`] then Err
    pub async fn try_send_message(
        &mut self,
        message_to_send: &Messages,
    ) -> Result<Messages, ControllerErrors> {
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
                    acceptor_id: _,
                    proposer_id: _,
                    accept_result: _,
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
/// loops infinitely adding every [`Messages`] received from receiver onto current_messages
async fn add_messages_to_queue(
    current_messages: Arc<Mutex<Vec<(Messages, UnboundedSender<Messages>)>>>,
    mut receiver: tokio::sync::mpsc::UnboundedReceiver<(Messages, UnboundedSender<Messages>)>,
) {
    loop {
        let incoming_message = receiver.recv().await.unwrap();
        current_messages.lock().await.push(incoming_message);
    }
}

#[derive(Debug, Clone)]
/// The struct which implements [`basic_paxos_lib::SendToAcceptors`] to give to the proposer.
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
