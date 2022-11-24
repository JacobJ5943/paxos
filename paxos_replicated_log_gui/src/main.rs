use flume::{Receiver, Sender};
use paxos_controllers::local_controller::{LocalMessageController, LocalMessageSender, Messages};
use tracing::Level;
use std::sync::atomic::Ordering;
use std::{
    collections::HashMap,
    sync::{atomic::AtomicUsize, Arc},
    time::Duration,
};

use basic_paxos_lib::{proposers::Proposer};
use clap::{command, value_parser, Arg, ArgAction};
use tokio::{
    sync::{Mutex, RwLock},
    time::timeout,
};

use crate::gui::run_gui;

mod gui;

fn get_matches() -> clap::ArgMatches {
    command!()
        .arg(
            Arg::new("ServerCount")
                .short('c')
                .long("server_count")
                .action(ArgAction::Append)
                .value_parser(value_parser!(usize))
                .required(false)
                .default_value("3"),
        )
        .get_matches()
}

#[derive(Debug, Default)]
struct Server {
    next_slot: Arc<AtomicUsize>, // why do I need to arc an atomic again?
    proposer_id: usize,
    prop: Arc<Mutex<basic_paxos_lib::proposers::Proposer>>,
    acceptor: Arc<Mutex<basic_paxos_lib::acceptors::Acceptor>>,
    decided_values: Arc<RwLock<Vec<usize>>>, // boo using arc so that the proposer can update this itself
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = get_matches();
    println!("Hello, world!");

    let subscriber = tracing_subscriber::FmtSubscriber::builder()
        // all spans/events with a level higher than TRACE (e.g, debug, info, warn, etc.)
        // will be written to stdout.
        .with_max_level(Level::INFO)
        // builds the subscriber.
        .finish();

    tracing::subscriber::set_global_default(subscriber).unwrap();

    let mut servers = Vec::new();
    for server_identifier in 0..*args.get_one("ServerCount").unwrap() {
        servers.push(Server {
            proposer_id: server_identifier,
            prop: Arc::new(Mutex::new(Proposer::new(server_identifier))),
            ..Default::default()
        });
    }
    let tokio_runtime = Arc::new(tokio::runtime::Runtime::new().unwrap());

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

    let server_count = servers.len();
    let GuiListenerResult{receive_frames, send_message_indecies, propose_value_sender} = create_gui_listener(
        &tokio_runtime,
        local_message_controller,
        local_message_sender,
        servers,
    );

    //let gui_join_handle = tokio::spawn(async {
    //gui::run_gui(prop_clone,acpt_clone).await;
    //});

    /*
    let _tokio_join_handle = std::thread::spawn(move || {
        tokio_runtime.block_on(run_tokio_things(
            shared_acceptor,
            acceptor_network_vec,
            local_proposer,
            sender_client,
            my_port,
        ))
    }); */

    run_gui(
        receive_frames,
        send_message_indecies,
        server_count,
        propose_value_sender,
    );

    println!("This should go through");

    //let tokio_result = tokio_join_handle.join();

    Ok(())
}

pub struct ServerFrame {
    prop_debug: String,
    acceptor_debug: String,
    decided_values: Vec<usize>,
}
// This is because I can't have tokio with egui boo
pub struct Frame {
    servers: Vec<ServerFrame>,
    waiting_messages: Vec<Messages>,
}

struct GuiListenerResult {
receive_frames:Receiver<Frame>, send_message_indecies:Sender<Vec<Messages>>, propose_value_sender:Sender<(usize, usize)>
}

fn create_gui_listener(
    tokio_runtime: &tokio::runtime::Runtime,
    local_message_controller: LocalMessageController,
    local_message_sender: LocalMessageSender,
    servers: Vec<Server>,
) -> 
    GuiListenerResult
 {
    let (send_frame, request_frame) = flume::bounded(1);
    let (send_message_indecies, receive_message_indecies) = flume::bounded(1);
    let (send_propose_value, receive_propose_value) = flume::unbounded(); // You can only click one button at a time
    tokio_runtime.spawn(the_function_that_actually_sends_the_messages(
        local_message_controller,
        send_frame,
        receive_message_indecies,
        receive_propose_value,
        servers,
        local_message_sender,
    ));
    //GuiListenerResult{request_frame, send_message_indecies, send_propose_value)
        GuiListenerResult { receive_frames:  request_frame, send_message_indecies, propose_value_sender: send_propose_value }
}

async fn update_decided_values(
    servers: &[Server],
    receive_decided_values: &Receiver<(usize, usize)>,
) {
    let mut decided_values_received = Vec::new();
    while let Ok((slot, decided_value)) = receive_decided_values.try_recv() {
        decided_values_received.push((slot, decided_value));
    }

    for server in servers.iter() {
        if !decided_values_received.is_empty() {
            let mut server_decided_values = dbg!(server.decided_values.write().await);
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

async fn create_server_frames(
    servers: &[Server],
    receive_decided_values: &Receiver<(usize, usize)>,
) -> Vec<ServerFrame> {
    let mut server_frames: Vec<ServerFrame> = Vec::new();

    update_decided_values(servers, receive_decided_values).await;

    for server in servers.iter() {
        let prop_debug = timeout(Duration::from_millis(10), async {
            format!("{:#?}", server.prop.lock().await)
        })
        .await
        .unwrap_or_else(|_err| "Proposing value currently".to_string());

        let acceptor_debug = format!("{:#?}", server.acceptor.lock().await); // Acceptor should never be locked for an extended period of time so I'm not concerned about this not having a timeout

        let decided_values: Vec<usize> = server.decided_values.read().await.clone();
        server_frames.push(ServerFrame {
            prop_debug,
            acceptor_debug,
            decided_values,
        })
    }
    server_frames
}
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
        let server_frames = create_server_frames(&servers, &receive_decided_values).await;
        // Get info on all of the serversQ
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
                spawn_and_handle_propose_value(&server_identifiers, &servers, proposer_id, total_acceptor_count, &local_message_sender, &send_decided_values, value);
            }
            Err(try_receive_error) => {
                match try_receive_error {
                    flume::TryRecvError::Empty => (),
                    flume::TryRecvError::Disconnected => todo!(), // This shouldn't happen unless the gui gets disconneted
                }
            }
        }
    }


    fn spawn_and_handle_propose_value(server_identifiers: &[usize], servers: &[Server], proposer_id:usize, total_acceptor_count: usize, local_message_sender: &LocalMessageSender, send_decided_values: &Sender<(usize, usize)>, proposing_value:usize) {
                let mut server_identifier_cloned = server_identifiers.to_owned();
                // I'm not a fan of how if this fails there won't be any indication
                // Right now if the there is no retry logic if another proposal is let through with a highest slot
                let matching_server = servers
                    .iter()
                    .find(|server| server.proposer_id == proposer_id)
                    .expect("proposer_id to be a valid proposer");
                let propser_to_propose = matching_server.prop.clone();
                let acceptor_count_cloned = total_acceptor_count;
                let local_message_sender_cloned = local_message_sender.clone();
                let decided_values_cloned: Arc<RwLock<Vec<usize>>> =
                    matching_server.decided_values.clone();
                let next_slot_cloned = matching_server.next_slot.clone();
                let send_decided_values_cloned = send_decided_values.clone();
                tokio::spawn(async move {
                    let mut proposer_to_propose = propser_to_propose.lock().await;

                    let propsing_slot = next_slot_cloned.load(Ordering::SeqCst);
                    let propose_result = proposer_to_propose
                        .propose_value(
                            proposing_value,
                            propsing_slot,
                            &local_message_sender_cloned,
                            &mut server_identifier_cloned,
                            acceptor_count_cloned,
                        )
                        .await;
                        dbg!(&propose_result);
                    match propose_result {
                        Ok(decided_value) => {
                            send_decided_values_cloned
                                .send_async((propsing_slot, decided_value))
                                .await
                                .unwrap();

                            let next_slot_cloned_value = next_slot_cloned.load(Ordering::SeqCst);
                            let decided_values_len = decided_values_cloned.read().await.len();

                            match next_slot_cloned_value.cmp(&decided_values_len) {
                                std::cmp::Ordering::Equal => decided_values_cloned.write().await.push(decided_value),
                                std::cmp::Ordering::Less => debug_assert!(decided_values_cloned.read().await.get(propsing_slot).unwrap() == &decided_value),
                                std::cmp::Ordering::Greater => unimplemented!("TODO I don't know what this case is")
                            }
                            let result = next_slot_cloned.compare_exchange(
                                propsing_slot,
                                propsing_slot + 1,
                                Ordering::SeqCst,
                                Ordering::SeqCst,
                            );
                            match result {
                                Ok(_) => (),
                                Err(_read_value) => unimplemented!(), // with a proposer being locked this whole time I'm not sure this can happen
                            }
                        }

                        Err(proposing_error) => {
                            dbg!(&proposing_error);
                            match proposing_error {
                                basic_paxos_lib::proposers::ProposingErrors::NewSlot(
                                    highest_slot_returned,
                                    _highest_ballot_returned,
                                ) => {
                                    let mut current_value = next_slot_cloned.load(Ordering::SeqCst);
                                    while let Err(current_value_returned) = dbg!(next_slot_cloned
                                        .compare_exchange_weak(
                                            current_value,
                                            *highest_slot_returned,
                                            Ordering::SeqCst,
                                            Ordering::SeqCst
                                        ))
                                    {
                                        current_value = current_value_returned;
                                    }
                                    dbg!(*highest_slot_returned);
                                }
                                basic_paxos_lib::proposers::ProposingErrors::NetworkError => {
                                    panic!("network error in local program")
                                }
                            };
                            // Do somethign better later
                        }
                    }
                });
    }
}
