#[cfg(test)]
mod tests {
    use std::{collections::HashMap, sync::Arc};
    use tokio::sync::Mutex;

    use basic_paxos_lib::{
        acceptors::Acceptor,
        proposers::{Proposer, ProposingErrors},
        HighestBallotPromised, HighestSlotPromised,
    };
    use paxos_controllers::local_controller::{ControllerErrors, LocalMessageController, Messages};
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
        let acceptors = acceptors
            .into_iter()
            .fold(HashMap::new(), |mut acc, (key, value)| {
                acc.insert(key, Arc::new(Mutex::new(value)));
                acc
            });
        let (mut local_controller, local_message_sender) =
            LocalMessageController::new(acceptors.clone());

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
            &Messages::PromiseRequest {
                acceptor_id: 1,
                proposer_id: 1,
                slot_num: 0,
                ballot_num: 1,
            },
            &mut local_controller,
            100,
        )
        .await
        .unwrap();
        let _promise_response_message = send_message_and_response(
            &Messages::PromiseRequest {
                acceptor_id: 2,
                proposer_id: 1,
                slot_num: 0,
                ballot_num: 1,
            },
            &mut local_controller,
            100,
        )
        .await
        .unwrap();
        let _promise_response_message = send_message_and_response(
            &Messages::PromiseRequest {
                acceptor_id: 3,
                proposer_id: 1,
                slot_num: 0,
                ballot_num: 1,
            },
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
            &Messages::AcceptRequest {
                acceptor_id: 1,
                proposer_id: 1,
                ballot_num: 1,
                slot_num: 0,
                value: 7,
            },
            &mut local_controller,
            100,
        )
        .await
        .unwrap();
        let _response_message = send_message_and_response(
            &Messages::AcceptRequest {
                acceptor_id: 2,
                proposer_id: 1,
                ballot_num: 1,
                slot_num: 0,
                value: 7,
            },
            &mut local_controller,
            100,
        )
        .await
        .unwrap();
        let _response_message = send_message_and_response(
            &Messages::AcceptRequest {
                acceptor_id: 3,
                proposer_id: 1,
                ballot_num: 1,
                slot_num: 0,
                value: 7,
            },
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
        let acceptors = acceptors
            .into_iter()
            .fold(HashMap::new(), |mut acc, (key, value)| {
                acc.insert(key, Arc::new(Mutex::new(value)));
                acc
            });
        let (mut local_controller, local_message_sender) =
            LocalMessageController::new(acceptors.clone());

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
            &Messages::PromiseRequest {
                acceptor_id: 3,
                proposer_id: 1,
                slot_num: 0,
                ballot_num: 1,
            },
            &mut local_controller,
            100,
        )
        .await;
        let _response_message = send_message_and_response(
            &Messages::PromiseRequest {
                acceptor_id: 4,
                proposer_id: 1,
                slot_num: 0,
                ballot_num: 1,
            },
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
            &Messages::AcceptRequest {
                acceptor_id: 3,
                proposer_id: 1,
                slot_num: 0,
                ballot_num: 1,
                value: 7,
            },
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
            &Messages::PromiseRequest {
                acceptor_id: 4,
                proposer_id: 2,
                slot_num: 0,
                ballot_num: 1,
            },
            &mut local_controller,
            100,
        )
        .await;
        let _response_message = send_message_and_response(
            &Messages::PromiseRequest {
                acceptor_id: 5,
                proposer_id: 2,
                slot_num: 0,
                ballot_num: 1,
            },
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
            &Messages::AcceptRequest {
                acceptor_id: 5,
                proposer_id: 2,
                slot_num: 0,
                ballot_num: 1,
                value: 3,
            },
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
            &Messages::AcceptRequest {
                acceptor_id: 4,
                proposer_id: 1,
                slot_num: 0,
                ballot_num: 1,
                value: 7,
            },
            &mut local_controller,
            100,
        )
        .await;
        let _response_message = send_message_and_response(
            &Messages::AcceptRequest {
                acceptor_id: 5,
                proposer_id: 1,
                slot_num: 0,
                ballot_num: 1,
                value: 7,
            },
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
            &Messages::PromiseRequest {
                acceptor_id: 3,
                proposer_id: 1,
                slot_num: 0,
                ballot_num: 2,
            },
            &mut local_controller,
            100,
        )
        .await;
        let _response_message = send_message_and_response(
            &Messages::PromiseRequest {
                acceptor_id: 4,
                proposer_id: 1,
                slot_num: 0,
                ballot_num: 2,
            },
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
            &Messages::AcceptRequest {
                acceptor_id: 4,
                proposer_id: 1,
                slot_num: 0,
                ballot_num: 2,
                value: 7,
            },
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
            &Messages::AcceptRequest {
                acceptor_id: 3,
                proposer_id: 1,
                slot_num: 0,
                ballot_num: 2,
                value: 7,
            },
            &mut local_controller,
            100,
        )
        .await
        .unwrap();
        let _response_message = send_message_and_response(
            &Messages::AcceptRequest {
                acceptor_id: 4,
                proposer_id: 2,
                slot_num: 0,
                ballot_num: 1,
                value: 3,
            },
            &mut local_controller,
            100,
        )
        .await
        .unwrap();
        let _response_message = send_message_and_response(
            &Messages::AcceptRequest {
                acceptor_id: 3,
                proposer_id: 2,
                slot_num: 0,
                ballot_num: 1,
                value: 3,
            },
            &mut local_controller,
            100,
        )
        .await
        .unwrap();

        // TODO These both will timeout since I didn't let their accept responses return a quorum.
        timeout(Duration::from_millis(100), proposing_1_join_handle)
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

        let acceptors = acceptors
            .into_iter()
            .fold(HashMap::new(), |mut acc, (key, value)| {
                acc.insert(key, Arc::new(Mutex::new(value)));
                acc
            });
        let (mut local_controller, local_message_sender) =
            LocalMessageController::new(acceptors.clone());

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
            &Messages::PromiseRequest {
                acceptor_id: 1,
                proposer_id: 1,
                slot_num: 0,
                ballot_num: 1,
            },
            &mut local_controller,
            100,
        )
        .await
        .unwrap();
        let _response_message = send_message_and_response(
            &Messages::PromiseRequest {
                acceptor_id: 2,
                proposer_id: 1,
                slot_num: 0,
                ballot_num: 1,
            },
            &mut local_controller,
            100,
        )
        .await
        .unwrap();
        let _response_message = send_message_and_response(
            &Messages::PromiseRequest {
                acceptor_id: 3,
                proposer_id: 1,
                slot_num: 0,
                ballot_num: 1,
            },
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
