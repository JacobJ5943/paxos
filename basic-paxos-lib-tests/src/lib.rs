use async_trait::async_trait;
use basic_paxos_lib::{
    acceptors::{AcceptedValue, Acceptor},
    HighestBallotPromised, HighestSlotPromised, PromiseReturn, SendToAcceptors,
};
use std::{collections::HashMap, sync::Arc};
use tokio::sync::{
    mpsc::{error::SendError, UnboundedSender},
    Mutex,
};

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
pub struct LocalMessageController {
    pub acceptors: HashMap<usize, Acceptor>, // <AcceptorIdentifier, Acceptor>
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
        acceptors: HashMap<usize, Acceptor>,
    ) -> (
        Self,
        tokio::sync::mpsc::UnboundedSender<(Messages, UnboundedSender<Messages>)>,
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
            sender,
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
                    let response = self.acceptors.get_mut(acceptor_id).unwrap().accept(
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
                    let response = self.acceptors.get_mut(acceptor_id).unwrap().promise(
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
    sender_to_controller: UnboundedSender<(Messages, UnboundedSender<Messages>)>, // The sender is so that the response can also be controlled
}

#[async_trait]
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

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use crate::{ControllerErrors, LocalMessageController, LocalMessageSender, Messages};
    use basic_paxos_lib::{
        acceptors::Acceptor,
        proposers::{Proposer, ProposingErrors},
        HighestBallotPromised, HighestSlotPromised,
    };
    use std::time::Duration;
    use tokio::time::{sleep, timeout};

    // This is why it needs to be a macro since expected response can be different
    async fn send_message_and_response(
        message_to_send: &Messages,
        local_message_controller: &mut LocalMessageController,
        timeout_millis: u64,
    ) -> Result<Messages, ControllerErrors> {
        let request_response_message = timeout(
            std::time::Duration::from_millis(100),
            local_message_controller.try_send_message(message_to_send),
        )
        .await
        .unwrap()
        .unwrap();

        let request_response_message_again = timeout(
            std::time::Duration::from_millis(timeout_millis),
            local_message_controller.try_send_message(&request_response_message),
        )
        .await
        .unwrap();
        sleep(std::time::Duration::from_millis(timeout_millis)).await;

        request_response_message_again
    }

    #[test]
    fn it_works() {
        let result = 2 + 2;
        assert_eq!(result, 4);
    }

    #[tokio::test]
    async fn test_acceptors_initial_propose() {
        let mut acceptors = HashMap::new();
        acceptors.insert(1, Acceptor::default());
        acceptors.insert(2, Acceptor::default());
        acceptors.insert(3, Acceptor::default());
        let (mut local_controller, sender) = LocalMessageController::new(acceptors.clone());
        let local_message_sender = LocalMessageSender {
            sender_to_controller: sender,
        };

        let mut acceptor_ids: Vec<usize> = acceptors.keys().copied().collect();

        let acceptor_count = acceptor_ids.len();

        let proposing_join_handle = tokio::spawn(async move {
            let mut proposers: HashMap<usize, Proposer> = HashMap::new();
            proposers.insert(1, Proposer::new(1));
            proposers.insert(2, Proposer::new(2));
            proposers.insert(3, Proposer::new(3));
            let proposer = proposers.get_mut(&1).unwrap();
            let propose_result = proposer
                .propose_value(
                    7,
                    0,
                    &local_message_sender,
                    &mut acceptor_ids,
                    acceptor_count,
                )
                .await;

            assert!(&propose_result.is_ok(), "{:?}", propose_result.unwrap_err());

            assert_eq!(propose_result.unwrap(), 7);
        });

        sleep(Duration::from_millis(100)).await;
        assert_eq!(
            local_controller.current_messages.lock().await.len(),
            3,
            "Not all messages have completed"
        );

        let _promise_response_message = send_message_and_response(
            &Messages::PromiseRequest(1, 1, 0, 1),
            &mut local_controller,
            100,
        )
        .await
        .unwrap();
        let _promise_response_message = send_message_and_response(
            &Messages::PromiseRequest(2, 1, 0, 1),
            &mut local_controller,
            100,
        )
        .await
        .unwrap();
        let _promise_response_message = send_message_and_response(
            &Messages::PromiseRequest(3, 1, 0, 1),
            &mut local_controller,
            100,
        )
        .await
        .unwrap_err(); // The proposer would have reached a quorum and stopped listening

        assert_eq!(
            timeout(
                Duration::from_millis(100),
                local_controller.current_messages.lock()
            )
            .await
            .unwrap()
            .len(),
            3,
            "3 accept requests expected. Not all messages have completed"
        );

        let _response_message = send_message_and_response(
            &Messages::AcceptRequest(1, 1, 0, 1, 7),
            &mut local_controller,
            100,
        )
        .await
        .unwrap();
        let _response_message = send_message_and_response(
            &Messages::AcceptRequest(2, 1, 0, 1, 7),
            &mut local_controller,
            100,
        )
        .await
        .unwrap();
        let _response_message = send_message_and_response(
            &Messages::AcceptRequest(3, 1, 0, 1, 7),
            &mut local_controller,
            100,
        )
        .await
        .unwrap_err(); // This should be an error since the proposer would have already decided a value and stopped listening

        assert_eq!(
            timeout(
                Duration::from_millis(100),
                local_controller.current_messages.lock()
            )
            .await
            .unwrap()
            .len(),
            0,
            "Not all messages have completed"
        );

        timeout(Duration::from_millis(100), proposing_join_handle)
            .await
            .unwrap()
            .unwrap();
    }

    ///
    /// This is the test case where I think the only answer is leader election
    ///
    /// If I have 2 proposers(p1 and p2) and 3 acceptors(a3,a4,a5).  
    ///
    /// p1 proposes Value(7) so sends promise to all with a3 and a4 responding
    /// p1 sends accept to all with only a3 responding and accepting the Value(7)
    ///
    /// Now p2 proposes Value(3).  So p2 sends promise to all with a4 and a5 responding.  p2 will now see the higher ballot, but not the accepted value by a3.  p2 sends another promise with only a4 and a5 responding again.
    /// Now p2 sends accept to all with only 5 responding and accepting the Value(3)
    ///
    /// Now we're in a state where d is not responding with 2 acceptors up and still no decided value.
    /// Once p2 is back up a value will be decided
    #[tokio::test]
    async fn three_acceptors_two_proposers_one_acceptor_left_with_other_two_having_different_values(
    ) {
        let mut acceptors = HashMap::new();
        acceptors.insert(3, Acceptor::default());
        acceptors.insert(4, Acceptor::default());
        acceptors.insert(5, Acceptor::default());
        let (mut local_controller, sender) = LocalMessageController::new(acceptors.clone());
        let local_message_sender = LocalMessageSender {
            sender_to_controller: sender,
        };

        let mut acceptor_ids: Vec<usize> = acceptors.keys().copied().collect();

        let acceptor_count = acceptor_ids.len();

        let local_message_sender_cloned = local_message_sender.clone();
        let mut acceptor_ids_cloned = acceptor_ids.clone();
        let proposing_1_join_handle = tokio::spawn(async move {
            let mut proposer_1 = Proposer::new(1);
            let propose_result = proposer_1
                .propose_value(
                    7,
                    0,
                    &local_message_sender,
                    &mut acceptor_ids,
                    acceptor_count,
                )
                .await;
            assert!(&propose_result.is_ok(), "{:?}", propose_result.unwrap_err());
            assert_eq!(propose_result.unwrap(), 7);
        });

        let proposing_2_join_handle = tokio::spawn(async move {
            let mut proposer_2 = Proposer::new(2);
            let propose_result = proposer_2
                .propose_value(
                    3,
                    0,
                    &local_message_sender_cloned,
                    &mut acceptor_ids_cloned,
                    acceptor_count,
                )
                .await;
            assert!(&propose_result.is_ok(), "{:?}", propose_result.unwrap_err());
            assert_eq!(propose_result.unwrap(), 7);
        });

        sleep(Duration::from_millis(100)).await;
        assert_eq!(
            local_controller.current_messages.lock().await.len(),
            6,
            "6 promise requests expected. Not all messages have completed"
        );

        // First Proposer 1 proposes value 7
        println!("now prop 1 proposes 7");
        let _response_message = send_message_and_response(
            &Messages::PromiseRequest(3, 1, 0, 1),
            &mut local_controller,
            100,
        )
        .await;
        let _response_message = send_message_and_response(
            &Messages::PromiseRequest(4, 1, 0, 1),
            &mut local_controller,
            100,
        )
        .await;

        assert_eq!(
            timeout(
                Duration::from_millis(100),
                local_controller.current_messages.lock()
            )
            .await
            .unwrap()
            .len(),
            7,
            "4 promise requests and 3 accept requests expected. Not all messages have completed"
        );
        let _response_message = send_message_and_response(
            &Messages::AcceptRequest(3, 1, 0, 1, 7),
            &mut local_controller,
            100,
        )
        .await;

        assert_eq!(
            timeout(
                Duration::from_millis(100),
                local_controller.current_messages.lock()
            )
            .await
            .unwrap()
            .len(),
            6,
            "4 promise requests and 2 accept requests expected. Not all messages have completed"
        );

        // Next Proposer 2 proposes value 3
        println!("now prop 2 proposes 3");
        let _response_message = send_message_and_response(
            &Messages::PromiseRequest(4, 2, 0, 1),
            &mut local_controller,
            100,
        )
        .await;
        let _response_message = send_message_and_response(
            &Messages::PromiseRequest(5, 2, 0, 1),
            &mut local_controller,
            100,
        )
        .await;

        // The reason for not needing to up the ballot number is because the proposer id breaks ties and this is prop 2
        assert_eq!(
            timeout(
                Duration::from_millis(100),
                local_controller.current_messages.lock()
            )
            .await
            .unwrap()
            .len(),
            7,
            "2 promise requests and 5 accept requests expected. Not all messages have completed"
        );

        let _response_message = send_message_and_response(
            &Messages::AcceptRequest(5, 2, 0, 1, 3),
            &mut local_controller,
            100,
        )
        .await;

        assert_eq!(
            timeout(
                Duration::from_millis(100),
                local_controller.current_messages.lock()
            )
            .await
            .unwrap()
            .len(),
            6,
            "2 promise requests and 4 accept requests expected. Not all messages have completed"
        );

        // Now send the accept for prop 1

        let _response_message = send_message_and_response(
            &Messages::AcceptRequest(4, 1, 0, 1, 7),
            &mut local_controller,
            100,
        )
        .await;
        let _response_message = send_message_and_response(
            &Messages::AcceptRequest(5, 1, 0, 1, 7),
            &mut local_controller,
            100,
        )
        .await;

        assert_eq!(
            timeout(
                Duration::from_millis(100),
                local_controller.current_messages.lock()
            )
            .await
            .unwrap()
            .len(),
            7,
            "5 promise requests and 2 accept requests expected. Not all messages have completed"
        );

        // prop 1 should now try to promise again
        let _response_message = send_message_and_response(
            &Messages::PromiseRequest(3, 1, 0, 2),
            &mut local_controller,
            100,
        )
        .await;
        let _response_message = send_message_and_response(
            &Messages::PromiseRequest(4, 1, 0, 2),
            &mut local_controller,
            100,
        )
        .await;
        assert_eq!(
            timeout(
                Duration::from_millis(100),
                local_controller.current_messages.lock()
            )
            .await
            .unwrap()
            .len(),
            8,
            "3 promise requests and 5 accept requests expected. Not all messages have completed"
        );
        let _response_message = send_message_and_response(
            &Messages::AcceptRequest(4, 1, 0, 2, 7),
            &mut local_controller,
            100,
        )
        .await;

        assert_eq!(
            timeout(
                Duration::from_millis(100),
                local_controller.current_messages.lock()
            )
            .await
            .unwrap()
            .len(),
            7,
            "3 promise requests and 4 accept requests expected. Not all messages have completed"
        );

        let _response_message = send_message_and_response(
            &Messages::AcceptRequest(3, 1, 0, 2, 7),
            &mut local_controller,
            100,
        )
        .await
        .unwrap();
        let _response_message = send_message_and_response(
            &Messages::AcceptRequest(4, 2, 0, 1, 3),
            &mut local_controller,
            100,
        )
        .await
        .unwrap();
        let _response_message = send_message_and_response(
            &Messages::AcceptRequest(3, 2, 0, 1, 3),
            &mut local_controller,
            100,
        )
        .await
        .unwrap();

        // TODO These both will timeout since I didn't let their accept responses return a quorum.
        let _result = timeout(Duration::from_millis(100), proposing_1_join_handle)
            .await
            .unwrap()
            .unwrap();
        let _result = timeout(Duration::from_millis(100), proposing_2_join_handle).await;
    }

    #[tokio::test]
    async fn test_prop_value_when_acceptors_are_a_slot_ahead() {
        let mut acceptors = HashMap::new();
        acceptors.insert(
            1,
            Acceptor {
                promised_ballot_num: Some(3),
                promised_slot_num: 2,
                promised_node_identifier: 1,
                accepted_value: None,
            },
        );
        acceptors.insert(
            2,
            Acceptor {
                promised_ballot_num: Some(3),
                promised_slot_num: 2,
                promised_node_identifier: 1,
                accepted_value: None,
            },
        );
        acceptors.insert(
            3,
            Acceptor {
                promised_ballot_num: Some(3),
                promised_slot_num: 2,
                promised_node_identifier: 1,
                accepted_value: None,
            },
        );
        let (mut local_controller, sender) = LocalMessageController::new(acceptors.clone());
        let local_message_sender = LocalMessageSender {
            sender_to_controller: sender,
        };

        let mut acceptor_ids: Vec<usize> = acceptors.keys().copied().collect();

        let acceptor_count = acceptor_ids.len();

        let proposing_join_handle = tokio::spawn(async move {
            let mut proposers: HashMap<usize, Proposer> = HashMap::new();
            proposers.insert(1, Proposer::new(1));
            proposers.insert(2, Proposer::new(2));
            proposers.insert(3, Proposer::new(3));
            let proposer = proposers.get_mut(&1).unwrap();
            let propose_result = proposer
                .propose_value(
                    7,
                    0,
                    &local_message_sender,
                    &mut acceptor_ids,
                    acceptor_count,
                )
                .await;

            assert!(&propose_result.is_err(), "{:?}", propose_result.unwrap());

            let expected_err =
                ProposingErrors::NewSlot(HighestSlotPromised(2), HighestBallotPromised(Some(3)));

            assert_eq!(propose_result.unwrap_err(), expected_err);
        });

        sleep(Duration::from_millis(100)).await;
        assert_eq!(
            local_controller.current_messages.lock().await.len(),
            3,
            "Not all messages have completed"
        );

        let _response_message = send_message_and_response(
            &Messages::PromiseRequest(1, 1, 0, 1),
            &mut local_controller,
            100,
        )
        .await
        .unwrap();
        let _response_message = send_message_and_response(
            &Messages::PromiseRequest(2, 1, 0, 1),
            &mut local_controller,
            100,
        )
        .await
        .unwrap();
        let _response_message = send_message_and_response(
            &Messages::PromiseRequest(3, 1, 0, 1),
            &mut local_controller,
            100,
        )
        .await
        .unwrap_err(); // quorum was reached and so there is no sender listening

        assert_eq!(
            timeout(
                Duration::from_millis(100),
                local_controller.current_messages.lock()
            )
            .await
            .unwrap()
            .len(),
            0,
            "All messages should have completed"
        );
        proposing_join_handle.await.unwrap();
    }
}
