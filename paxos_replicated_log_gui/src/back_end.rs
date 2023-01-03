use basic_paxos_lib::proposers::Proposer;
use flume::{Receiver, Sender};
use paxos_controllers::local_controller::{LocalMessageController, LocalMessageSender, Messages};
use std::collections::HashMap;
use std::sync::atomic::Ordering;
use std::sync::{atomic::AtomicUsize, Arc};
use tokio::{
    runtime::Runtime,
    sync::{Mutex, RwLock},
};

use crate::frames::{self, Frame};

#[derive(Debug, Default)]
pub struct Server {
    pub next_slot: Arc<AtomicUsize>,
    pub proposer_id: usize,
    pub prop: Arc<Mutex<basic_paxos_lib::proposers::Proposer>>,
    pub acceptor: Arc<Mutex<basic_paxos_lib::acceptors::Acceptor>>,
    pub decided_values: Arc<RwLock<Vec<usize>>>,
}

pub struct GuiListenerResult {
    /// The receiver for the front end to receive [`Frame`][`crate::frames::Frame`] structs from the backend.
    receive_frames: Receiver<Frame>,
    /// The Sender for the front end to decide when messages are sent
    send_message_indices: Sender<Vec<Messages>>,
    /// The sender to queue up proposals. (proposer_id, proposing_value)
    propose_value_sender: Sender<(usize, usize)>,
}

/// Creates all of the Producers and Consumers needed for the frontend
pub(crate) fn create_gui_listener(
    tokio_runtime: &tokio::runtime::Runtime,
    local_message_controller: LocalMessageController,
    local_message_sender: LocalMessageSender,
    servers: Vec<Server>,
) -> GuiListenerResult {
    let (send_frame, request_frame) = flume::bounded(1);
    let (send_message_indices, receive_message_indices) = flume::bounded(1);
    let (send_propose_value, receive_propose_value) = flume::unbounded(); // You can only click one button at a time
    tokio_runtime.spawn(the_function_that_actually_sends_the_messages(
        local_message_controller,
        send_frame,
        receive_message_indices,
        receive_propose_value,
        servers,
        local_message_sender,
    ));
    GuiListenerResult {
        receive_frames: request_frame,
        send_message_indices,
        propose_value_sender: send_propose_value,
    }
}

/// This is the function which will tell the other servers when a value has been decided
/// This is a part of paxos where I have seen a few different ideas.
/// Since this seemed pretty use case dependant I just picked the most straight forward for this use case.
///
/// This is required so that the other servers learn about the values in a timely manner.  
/// If this didn't occur the other servers would not update their decided_values until they propose a value.
///
/// This also avoids writing that catchup procedure since it depends on what's storing the decided values.  
/// In this case [`Server::decided_values`]
pub(crate) async fn update_decided_values(
    servers: &[Server],
    receive_decided_values: &Receiver<(usize, usize)>,
) {
    let mut decided_values_received = Vec::new();
    while let Ok((slot, decided_value)) = receive_decided_values.try_recv() {
        decided_values_received.push((slot, decided_value));
    }

    for server in servers.iter() {
        if !decided_values_received.is_empty() {
            let mut server_decided_values = server.decided_values.write().await;
            for (slot, value) in decided_values_received.iter() {
                match server_decided_values.get(*slot) {
                    Some(already_decided_value) => {
                        assert_eq!(already_decided_value, value, "A value: {} has already been decided for slot {}.  New Value received: {}",already_decided_value, slot, value);
                    }
                    None => {
                        server_decided_values.push(*value);
                    }
                }
            }
        }
    }
}

/// This updates next_slot based on what values have been decided
async fn update_next_slot(
    next_slot: Arc<AtomicUsize>,
    decided_values: Arc<RwLock<Vec<usize>>>,
    decided_value: usize,
    proposing_slot: usize,
) {
    let next_slot_value = next_slot.load(Ordering::SeqCst);
    let decided_values_len = decided_values.read().await.len();

    match next_slot_value.cmp(&decided_values_len) {
        std::cmp::Ordering::Equal => decided_values.write().await.push(decided_value),
        std::cmp::Ordering::Less => debug_assert!(
            decided_values.read().await.get(proposing_slot).unwrap() == &decided_value
        ),
        std::cmp::Ordering::Greater => {
            unimplemented!("TODO I don't know what this case is")
        }
    }
    let result = next_slot.compare_exchange(
        proposing_slot,
        proposing_slot + 1,
        Ordering::SeqCst,
        Ordering::SeqCst,
    );
    match result {
        Ok(_) => (),
        Err(_read_value) => unimplemented!(), // with a proposer being locked this whole time I'm not sure this can happen
    }
}

/// This is the backed event loop
///
/// It's very important that send_frames be a bounded channel as the backend will send as many frames as it can.
/// If it's a bounded(1) then the backend will only send a new frame when the front end has updated.  
///
/// receive_propose_value is a Receiver<(proposer_id, proposing_valued)>
// Currently this will create a frame and then wait until the frontend has updated to send it
// This means that after the front end updates it's getting the frame that is generated not at that point of update, but the point of the previous update
async fn the_function_that_actually_sends_the_messages(
    mut local_message_controller: LocalMessageController,
    send_frames: Sender<Frame>,
    messages_to_send: Receiver<Vec<Messages>>,
    receive_propose_value: Receiver<(usize, usize)>,
    servers: Vec<Server>,
    local_message_sender: LocalMessageSender,
) {
    let total_acceptor_count = servers.len();
    let server_identifiers = servers
        .iter()
        .map(|server| server.proposer_id)
        .collect::<Vec<usize>>();

    let (send_decided_values, receive_decided_values) = flume::unbounded::<(usize, usize)>();
    loop {
        let server_frames = frames::create_server_frames(&servers, &receive_decided_values).await;
        // Get info on all of the servers
        let frame = Frame {
            servers: server_frames,
            waiting_messages: local_message_controller
                .current_messages
                .lock()
                .await
                .iter()
                .map(|(message, _sender)| message.clone())
                .collect(),
        };
        send_frames.send_async(frame).await.unwrap();
        let messages_to_send = messages_to_send.recv_async().await.expect(
            "That I required the gui thread to always respond with some vec that can be empty",
        );
        for message in messages_to_send.into_iter().rev() {
            match local_message_controller.try_send_message(&message).await {
                Ok(_) => (),
                Err(err) => {
                    dbg!("failed_to_send_message with err:{:?}", err);
                }
            };
        }

        let propose_value = receive_propose_value.try_recv();
        match propose_value {
            Ok((proposer_id, value)) => {
                spawn_and_handle_propose_value(
                    &server_identifiers,
                    &servers,
                    proposer_id,
                    total_acceptor_count,
                    &local_message_sender,
                    &send_decided_values,
                    value,
                );
            }
            Err(try_receive_error) => {
                match try_receive_error {
                    flume::TryRecvError::Empty => (),
                    flume::TryRecvError::Disconnected => todo!(), // This shouldn't happen unless the gui gets disconnected
                }
            }
        }
    }
}

/// This function will spawn a new Task which will propose the value.
/// After that it will update matching_server.next_slot if applicable
fn spawn_and_handle_propose_value(
    server_identifiers: &[usize],
    servers: &[Server],
    proposer_id: usize,
    total_acceptor_count: usize,
    local_message_sender: &LocalMessageSender,
    send_decided_values: &Sender<(usize, usize)>,
    proposing_value: usize,
) {
    let acceptor_count = total_acceptor_count;
    let local_message_sender = local_message_sender.clone();
    let send_decided_values = send_decided_values.clone();
    let mut server_identifier = server_identifiers.to_owned();

    let matching_server = servers
        .iter()
        .find(|server| server.proposer_id == proposer_id)
        .expect("proposer_id to be a valid proposer");
    let decided_values: Arc<RwLock<Vec<usize>>> = matching_server.decided_values.clone();
    let next_slot = matching_server.next_slot.clone();
    let proposer_to_propose = matching_server.prop.clone();

    tokio::spawn(async move {
        let mut proposer_to_propose = proposer_to_propose.lock().await;

        let proposing_slot = next_slot.load(Ordering::SeqCst);
        let propose_result = proposer_to_propose
            .propose_value(
                proposing_value,
                proposing_slot,
                &local_message_sender,
                &mut server_identifier,
                acceptor_count,
            )
            .await;
        match propose_result {
            Ok(decided_value) => {
                send_decided_values
                    .send_async((proposing_slot, decided_value))
                    .await
                    .unwrap();
                update_next_slot(next_slot, decided_values, decided_value, proposing_slot).await;
            }

            Err(proposing_error) => {
                match proposing_error {
                    basic_paxos_lib::proposers::ProposingErrors::NewSlot(
                        highest_slot_returned,
                        _highest_ballot_returned,
                    ) => {
                        let mut current_value = next_slot.load(Ordering::SeqCst);
                        while let Err(current_value_returned) = next_slot.compare_exchange_weak(
                            current_value,
                            *highest_slot_returned,
                            Ordering::SeqCst,
                            Ordering::SeqCst,
                        ) {
                            current_value = current_value_returned;
                        }
                    }
                    basic_paxos_lib::proposers::ProposingErrors::NetworkError => {
                        panic!("network error in local program")
                    }
                };
            }
        }
    });
}

/// Creates and starts the back_end returning everything needed for the front_end
pub fn create_and_start_backend(
    server_count: usize,
    tokio_runtime: &Runtime,
) -> (
    Receiver<Frame>,
    Sender<Vec<Messages>>,
    Sender<(usize, usize)>,
) {
    let mut servers = Vec::new();
    for server_identifier in 0..server_count {
        servers.push(Server {
            proposer_id: server_identifier,
            prop: Arc::new(Mutex::new(Proposer::new(server_identifier))),
            ..Default::default()
        });
    }

    //let tokio_runtime = Arc::new(tokio::runtime::Runtime::new().unwrap());
    let mut acceptor_hashmap = HashMap::new();
    let proposer_hashmap = RwLock::new(HashMap::new());
    for server in servers.iter() {
        let proposer = server.prop.clone();
        let proposer_id = proposer.blocking_lock().node_identifier;
        acceptor_hashmap.insert(proposer_id, server.acceptor.clone());
        proposer_hashmap
            .blocking_write()
            .insert(proposer_id, proposer);
    }

    let (local_message_controller, local_message_sender) =
        tokio_runtime.block_on(async { LocalMessageController::new(acceptor_hashmap) });

    let GuiListenerResult {
        receive_frames,
        send_message_indices,
        propose_value_sender,
    } = crate::back_end::create_gui_listener(
        tokio_runtime,
        local_message_controller,
        local_message_sender,
        servers,
    );

    (receive_frames, send_message_indices, propose_value_sender)
}
