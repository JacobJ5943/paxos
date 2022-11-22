use std::{collections::HashMap, sync::{Arc}};

use basic_paxos_lib::{HighestBallotPromised, PromiseReturn, acceptors::{AcceptedValue, Acceptor}, HighestSlotPromised, SendToAcceptors};
use tokio::sync::{Mutex, mpsc::{UnboundedSender, error::SendError}};

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum Messages {
    AcceptRequest(usize, usize, usize, usize, usize), // AcceptorIdentifier, proposer_id, slot_num, ballot_num, value
    AcceptResponse(
        usize,
        usize,
        Result<AcceptedValue, (HighestSlotPromised, HighestBallotPromised)>,
    ), // acceptor_id, proposer_id
    PromiseRequest(usize, usize, usize, usize), // AcceptorIdentifier, slot_num, ballot_num, proposer_identifier
    PromiseResponse(usize, usize, Result<Option<AcceptedValue>, PromiseReturn>), // acceptor_id, proposer_id
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
    pub fn new(
        acceptors: HashMap<usize, Arc<Mutex<Acceptor>>>,
    ) -> (
        Self,
        LocalMessageSender,
    ) {
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
            LocalMessageSender{ sender_to_controller: sender },
        )
    }
    pub async fn try_send_message(
        &mut self,
        message_to_send: &Messages,
    ) -> Result<Messages, ControllerErrors> {
        println!("trying to remove message {:?}", &message_to_send);
        let x = self
            .current_messages
            .lock()
            .await
            .iter()
            .enumerate()
            .filter(|(_, (message_in_vec, _))| message_in_vec == message_to_send)
            .map(|(index, _other)| index)
            .clone()
            .collect::<Vec<usize>>();

        if x.len() > 1 {
            unimplemented!(
                "Some day I'll allow for message retries in this.  Today is not that day"
            )
        } else if x.is_empty() {
            Err(ControllerErrors::MessageNotFound)
        } else {
            let (message, sender) = self.current_messages.lock().await.remove(x[0]);
            match &message {
                Messages::AcceptRequest(acceptor_id, proposer_id, slot_num, ballot_num, value) => {
                    // The lock is only for the duration of the accept which is not waiting on other messages so a deadlock cannot occur
                    let response = self.acceptors.get_mut(acceptor_id).unwrap().lock().await.accept(
                        *ballot_num,
                        *slot_num,
                        *proposer_id,
                        *value,
                    );
                    let accept_response =
                        Messages::AcceptResponse(*acceptor_id, *proposer_id, response);
                    self.current_messages
                        .lock()
                        .await
                        .push((accept_response.clone(), sender));
                    Ok(accept_response)
                }
                Messages::AcceptResponse(_acceptor_id, _proposer_id, _accept_response) => {
                    match sender.send(message.clone()) {
                        Ok(()) => Ok(message),
                        Err(send_error) => Err(ControllerErrors::from(send_error)),
                    }
                }
                Messages::PromiseRequest(acceptor_id, proposer_id, slot_num, ballot_num) => {
                    // The lock is only for the duration of the promise which is not waiting on other messages so a deadlock cannot occur
                    let response = self.acceptors.get_mut(acceptor_id).unwrap().lock().await.promise(
                        *ballot_num,
                        *slot_num,
                        *proposer_id,
                    );
                    let promise_response =
                        Messages::PromiseResponse(*acceptor_id, *proposer_id, response);
                    self.current_messages
                        .lock()
                        .await
                        .push((promise_response.clone(), sender));
                    Ok(promise_response)
                }
                Messages::PromiseResponse(_, _, _response) => {
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
            Messages::AcceptRequest(
                acceptor_identifier,
                proposer_identifier,
                slot_num,
                ballot_num,
                value,
            ),
            sender,
        ));
        assert!(send_result.is_ok());
        let result = receiver.recv().await.unwrap();

        match result {
            Messages::AcceptResponse(
                _acceptor_identifier,
                _proposer_identifier,
                accept_response,
            ) => accept_response,
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
            Messages::PromiseRequest(
                acceptor_identifier,
                proposer_identifier,
                slot_num,
                ballot_num,
            ),
            sender,
        ));
        assert!(send_result.is_ok());
        let result = receiver.recv().await.unwrap();

        match result {
            Messages::PromiseResponse(
                _acceptor_identifier,
                _proposer_identifier,
                promise_response,
            ) => promise_response,
            _ => panic!("Incorrect response from promise request"),
        }
    }
}