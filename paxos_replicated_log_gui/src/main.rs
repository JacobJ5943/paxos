use basic_paxos_lib::acceptors::AcceptedValue;
use basic_paxos_lib::HighestBallotPromised;
use color_eyre::owo_colors::OwoColorize;
use flume::{Receiver, Sender};
use paxos_controllers::local_controller::{LocalMessageController, LocalMessageSender, Messages};
use std::sync::atomic::Ordering;
use std::{
    borrow::Borrow,
    collections::HashMap,
    convert::Infallible,
    net::SocketAddr,
    sync::{atomic::AtomicUsize, Arc},
    time::Duration,
};

use async_trait::async_trait;
use axum::{
    extract,
    extract::{FromRequest, Json, RequestParts},
    http::{HeaderMap, Method, Uri},
    response::IntoResponse,
    routing::post,
    Extension, Router,
};
use axum_macros::debug_handler;
use basic_paxos_lib::{acceptors::Acceptor, proposers::Proposer, PromiseReturn};
use clap::{command, value_parser, Arg, ArgAction};
use hyper::{Body, Request};
use serde::{Deserialize, Serialize};
use tokio::{
    sync::{Mutex, RwLock},
    time::timeout,
};
use tracing::{info, instrument, Level};

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
    let mut proposer_hashmap = RwLock::new(HashMap::new());
    for server in servers.iter() {
        let proposer = server.prop.clone();
        let proposer_id = proposer.blocking_lock().node_identifier;
        acceptor_hashmap.insert(proposer_id, server.acceptor.clone());
        proposer_hashmap
            .blocking_write()
            .insert(proposer_id, proposer);
    }

    let (mut local_message_controller, local_message_sender) =
        tokio_runtime.block_on(async { LocalMessageController::new(acceptor_hashmap) });

    let server_count = servers.len().clone();
    let (receive_frames, send_message_idencies, propose_value_sender) = create_gui_listener(
        &tokio_runtime,
        local_message_controller,
        local_message_sender,
        servers,
    );

    //let gui_join_handle = tokio::spawn(async {
    //gui::run_gui(prop_clone,acpt_clone).await;
    //});

    let tokio_runtime_cloned = tokio_runtime.clone();
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
        tokio_runtime_cloned,
        receive_frames,
        send_message_idencies,
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

fn create_gui_listener(
    tokio_runtime: &tokio::runtime::Runtime,
    local_message_controller: LocalMessageController,
    local_message_sender: LocalMessageSender,
    servers: Vec<Server>,
) -> (
    Receiver<Frame>,
    Sender<Vec<Messages>>,
    Sender<(usize, usize)>,
) {
    let (send_frame, request_frame) = flume::bounded(1);
    let (send_message_indecies, receive_message_indecies) = flume::bounded(1);
    let (send_propose_value, receive_propose_value) = flume::bounded(1); // You can only click one button at a time
    tokio_runtime.spawn(the_function_that_actually_sends_the_messages(
        local_message_controller,
        send_frame,
        receive_message_indecies,
        receive_propose_value,
        servers,
        local_message_sender,
    ));
    (request_frame, send_message_indecies, send_propose_value)
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
        let mut server_frames: Vec<ServerFrame> = Vec::new();
        let mut decided_values_received = Vec::new();
        while let Ok((slot, decided_value)) = receive_decided_values.try_recv() {
            decided_values_received.push((slot, decided_value));
        }
        for server in servers.iter() {
            let prop_debug = timeout(Duration::from_millis(10), async {
                format!("{:#?}", server.prop.lock().await)
            })
            .await
            .unwrap_or_else(|err| "Proposing value currently".to_string());
            let acceptor_debug = format!("{:#?}", server.acceptor.lock().await); // Acceptor should never be locked for an extended period of time so I'm not concerned about this not having a timeout

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
            let decided_values: Vec<usize> = server.decided_values.read().await.clone();
            server_frames.push(ServerFrame {
                prop_debug,
                acceptor_debug,
                decided_values,
            })
        }
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
                let mut server_identifier_cloned = server_identifiers.clone();
                // I'm not a fan of how if this fails there won't be any indication
                // Right now if the there is no retry logic if another proposal is let through with a highest slot
                let matching_server = servers
                    .iter()
                    .find(|server| server.proposer_id == proposer_id)
                    .expect("proposer_id to be a valid proposer");
                let propser_to_propose = matching_server.prop.clone();
                let acceptor_count_cloned = total_acceptor_count.clone();
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
                            value,
                            propsing_slot,
                            &local_message_sender_cloned,
                            &mut server_identifier_cloned,
                            acceptor_count_cloned,
                        )
                        .await;
                    match propose_result {
                        Ok(decided_value) => {
                            send_decided_values_cloned
                                .send_async((propsing_slot, decided_value))
                                .await
                                .unwrap();
                            debug_assert!(
                                dbg!(next_slot_cloned.load(Ordering::SeqCst))
                                    == dbg!(decided_values_cloned.read().await.len())
                            ); // The + 1 is because of the way I select the slot in the loop thing. It's really late and I'm tired
                            decided_values_cloned.write().await.push(decided_value);
                            let result = next_slot_cloned.compare_exchange(
                                propsing_slot,
                                propsing_slot + 1,
                                Ordering::SeqCst,
                                Ordering::SeqCst,
                            );
                            match result {
                                Ok(_) => (),
                                Err(read_value) => unimplemented!(), // with a proposer being locked this whole time I'm not sure this can happen
                            }
                        }

                        Err(proposing_error) => {
                            dbg!(&proposing_error);
                            match proposing_error {
                                basic_paxos_lib::proposers::ProposingErrors::NewSlot(
                                    highest_slot_returned,
                                    highest_ballot_returned,
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
            Err(try_receive_error) => {
                match try_receive_error {
                    flume::TryRecvError::Empty => (),
                    flume::TryRecvError::Disconnected => todo!(), // This shouldn't happen unless the gui gets disconneted
                }
            }
        }
    }
    todo!()
}

#[derive(Deserialize, Serialize, Debug)]
struct AcceptRequestBody {
    proposing_ballot_num: usize,
    node_identifier: usize,
    value: usize,
}

#[derive(Debug, Serialize, Deserialize)]
struct AcceptedValueWrapper {
    accepted_value: AcceptedValue,
}

#[derive(Debug, Serialize, Deserialize)]
struct HighestBallotPromisedWrapper {
    highest_ballot_promised: HighestBallotPromised,
}

/// This is what the acceptors will receive accept requests on
#[instrument]
async fn accept(
    acceptor_state: Extension<Arc<Mutex<basic_paxos_lib::acceptors::Acceptor>>>,
    Json(request_body): extract::Json<AcceptRequestBody>,
) -> Json<Result<AcceptedValue, HighestBallotPromised>> {
    info!("accepting_value");
    let mut lock = acceptor_state.lock().await;
    drop(lock);
    /*
    let result = lock.accept(
        request_body.proposing_ballot_num,
        request_body.node_identifier,
        request_body.value,
    ); */
    todo!();
}

#[derive(Deserialize, Debug)]
struct ProposeValueResult {
    proposing_value: usize,
}

#[derive(Debug)]
struct AcceptorNetworkStruct {
    acceptor_port: usize,
}

/// The endpoint which the clients will call to propose a value.
#[debug_handler]
#[instrument]
async fn propose_value(
    Json(proposing_value): Json<ProposeValueResult>,
    Extension(acceptor_network_vec): Extension<Arc<RwLock<Vec<AcceptorNetworkStruct>>>>,
    Extension(local_proposer): Extension<Arc<Mutex<basic_paxos_lib::proposers::Proposer>>>,
    // I don't want this function to return until a value has been decided though so the single threaded thing wouldn't be a benefit necisarilyQ
) -> Json<Result<(), String>> {
    info!("proposing_value");
    let mut lock = local_proposer.lock().await;
    let mut acceptor_ports: Vec<usize> = acceptor_network_vec
        .try_read()
        .unwrap()
        .iter()
        .map(|acceptor_network| acceptor_network.acceptor_port)
        .collect();
    let acceptor_count: usize = acceptor_ports.len();
    let proposing_value = proposing_value.proposing_value;
    // let result: Result<(), usize> = lock
    //     .propose_value(
    //         proposing_value,
    //         &send_to_forward_server.borrow(),
    //         &mut acceptor_ports,
    //         acceptor_count,
    //     )
    //     .await;
    // match result {
    //     Ok(_) => Json(Ok(())),
    //     Err(other_accepted_value) => Json(Err(format! {"{other_accepted_value}"})),
    // }
    todo!()
}

#[derive(Deserialize, Debug, Serialize)]
struct PromiseRequest {
    ballot_num: usize,
    node_identifier: usize,
}

#[debug_handler]
/// The endpoint which acceptors will receive promises on
#[instrument]
async fn receive_promise(
    acceptor_state: Extension<Arc<Mutex<basic_paxos_lib::acceptors::Acceptor>>>,
    Json(request_body): extract::Json<PromiseRequest>,
) -> Json<Result<Option<AcceptedValue>, PromiseReturnWrapper>> {
    info!("receiving_promise");
    todo!()
}

#[derive(Debug, Serialize, Deserialize)] // These should be behind a feature flag probably
struct PromiseReturnWrapper(basic_paxos_lib::PromiseReturn);

impl IntoResponse for PromiseReturnWrapper {
    fn into_response(self) -> axum::response::Response {
        (axum::http::StatusCode::OK, self).into_response()
    }
}

struct MyExtractor {
    _method: Method,
    _path: String,
    _headers: HeaderMap,
    _query: String,
}

#[async_trait]
impl<B: Send> FromRequest<B> for MyExtractor {
    type Rejection = Infallible;

    async fn from_request(req: &mut RequestParts<B>) -> Result<Self, Infallible> {
        let method = req.extract::<Method>().await?;
        let path = req.extract::<Uri>().await?.path().to_owned();
        let headers = req.headers().clone();
        let query = headers
            .get("node-identifier")
            .map(|value| value.to_str().unwrap_or("node-identifier header not a str"))
            .unwrap_or_else(|| "failed to find node-identifier header")
            .to_string();
        Ok(MyExtractor {
            _method: method,
            _path: path,
            _headers: headers,
            _query: query,
        })
    }
}
