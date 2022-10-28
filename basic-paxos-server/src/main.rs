use basic_paxos_lib::acceptors::AcceptedValue;
use basic_paxos_lib::acceptors::HighestBallotPromised;
use std::{borrow::Borrow, collections::HashMap, convert::Infallible, net::SocketAddr, sync::Arc};

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
use hyper::{Body, Request, StatusCode};
use serde::{Deserialize, Serialize};
use tokio::sync::{Mutex, RwLock};
use tracing::{info, instrument, Level};

use crate::gui::run_gui;

mod gui;

fn get_matches() -> clap::ArgMatches {
    command!()
        .arg(
            Arg::new("MyPort")
                .short('p')
                .long("my-port")
                .required(true)
                .value_parser(value_parser!(u16))
                .action(ArgAction::Set),
        )
        .arg(
            Arg::new("AcceptorPorts")
                .short('o')
                .long("acceptors")
                .action(ArgAction::Append)
                .value_parser(value_parser!(usize))
                .required(true),
        )
        .get_matches()
}

async fn run_tokio_things(
    local_acceptor: Arc<Mutex<Acceptor>>,
    shared_state: Arc<Mutex<SharedState>>,
    acceptor_network_vec: Arc<RwLock<Vec<AcceptorNetworkStruct>>>,
    local_proposer: Arc<Mutex<Proposer>>,
    sender_client: Arc<SendToForwardServer>,
    my_port: u16,
) -> Result<(), ()> {
    let app = Router::new()
        .route("/accept", post(accept))
        .layer(Extension(local_acceptor.clone()))
        .layer(Extension(shared_state))
        .route("/proposevalue", post(propose_value))
        .layer(Extension(acceptor_network_vec))
        .layer(Extension(local_proposer))
        .layer(Extension(sender_client))
        .route("/promise", post(receive_promise))
        .layer(Extension(local_acceptor));

    let addr = SocketAddr::from(([127, 0, 0, 1], my_port));

    let server_join_handle = axum::Server::bind(&addr)
        .serve(app.into_make_service())
        .await;

    println!("What the fuck");

    Ok(())
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

    let my_port = *args.get_one::<u16>("MyPort").unwrap();
    let shared_state = Arc::new(Mutex::new(SharedState {}));

    let shared_acceptor = Arc::new(Mutex::new(basic_paxos_lib::acceptors::Acceptor::default()));

    let acceptor_network_vec: Vec<AcceptorNetworkStruct> = args
        .get_many("AcceptorPorts")
        .unwrap()
        .copied()
        .map(|acceptor_port| AcceptorNetworkStruct { acceptor_port })
        .collect();

    let test = acceptor_network_vec
        .iter()
        .map(|a| (a.acceptor_port, a.acceptor_port))
        .collect::<Vec<(usize, usize)>>();
    let mut forward_do_hickie: HashMap<usize, usize> = HashMap::new(); // Just doing this so I can test things as they are now
    for (x, y) in test.into_iter() {
        forward_do_hickie.insert(x, y);
    }
    let acceptor_network_vec = Arc::new(RwLock::new(acceptor_network_vec));

    let local_proposer = basic_paxos_lib::proposers::Proposer {
        current_highest_ballot: 0,
        node_identifier: my_port as usize,
    };
    let local_proposer = Arc::new(Mutex::new(local_proposer));

    let sender_client = Arc::new(SendToForwardServer {
        acceptors: forward_do_hickie,
    });

    let prop_clone = local_proposer.clone();
    let acpt_clone = shared_acceptor.clone();
    let tokio_runtime = Arc::new(tokio::runtime::Runtime::new().unwrap());
    //let gui_join_handle = tokio::spawn(async {
    //gui::run_gui(prop_clone,acpt_clone).await;
    //});

    let tokio_runtime_cloned = tokio_runtime.clone();
    let tokio_join_handle = std::thread::spawn(move || {
        tokio_runtime.block_on(run_tokio_things(
            shared_acceptor,
            shared_state,
            acceptor_network_vec,
            local_proposer,
            sender_client,
            my_port,
        ))
    });

    let result = run_gui(tokio_runtime_cloned, prop_clone, acpt_clone);

    println!("This should go through");

    //let tokio_result = tokio_join_handle.join();

    Ok(())
}

#[derive(Deserialize, Serialize, Debug)]
struct AcceptRequestBody {
    propsing_ballot_num: usize,
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
/// This will need to have access to the acceptors, but doesn't need access to anything else
/// TODO The return type should probably not be that
/// :
#[instrument]
async fn accept(
    acceptor_state: Extension<Arc<Mutex<basic_paxos_lib::acceptors::Acceptor>>>,
    Json(request_body): extract::Json<AcceptRequestBody>,
) -> Json<Result<AcceptedValue, HighestBallotPromised>> {
    info!("accepting_value");
    let mut lock = acceptor_state.lock().await;
    let result = lock.accept(
        request_body.propsing_ballot_num,
        request_body.node_identifier,
        request_body.value,
    );

    dbg!("fuck yeah we finished accepting with result {}", &result);
    dbg!(Json(result))
}

#[derive(Deserialize, Debug)]
struct ProposeValueResut {
    proposing_value: usize,
}

#[derive(Debug)]
struct AcceptorNetworkStruct {
    acceptor_port: usize,
}

/// The endpoint which the clients will call to propose a value.
/// This will need access to proposers and possibly it's acceptor.  
/// The reason for this is instead of calling accept to this node's port
/// it could just call the accept function
#[debug_handler]
#[instrument]
async fn propose_value(
    Json(proposing_value): Json<ProposeValueResut>,
    Extension(acceptor_network_vec): Extension<Arc<RwLock<Vec<AcceptorNetworkStruct>>>>,
    Extension(local_proposer): Extension<Arc<Mutex<basic_paxos_lib::proposers::Proposer>>>,
    Extension(send_to_forward_server): Extension<Arc<SendToForwardServer>>, // I think it's fine for this to lock the whole proposer since all proposed values should be sequential for the node
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
    let result: Result<(), usize> = lock
        .propose_value(
            proposing_value,
            &send_to_forward_server.borrow(),
            &mut acceptor_ports,
            acceptor_count,
        )
        .await;
    match result {
        Ok(_) => Json(Ok(())),
        Err(other_accepted_value) => Json(Err(format! {"{other_accepted_value}"})),
    }
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
    info!("recieing_promise");
    let result = acceptor_state
        .lock()
        .await
        .promise(request_body.ballot_num, request_body.node_identifier);
    dbg!(
        "fuck yeah we finished processing the promise with result {}",
        &result
    );

    Json(result.map_err(|err| dbg!(dbg!(PromiseReturnWrapper(err)))))
}

#[derive(Debug, Serialize, Deserialize)] // These should be behind a featuer flag probably
struct PromiseReturnWrapper(basic_paxos_lib::PromiseReturn);

impl IntoResponse for PromiseReturnWrapper {
    fn into_response(self) -> axum::response::Response {
        (axum::http::StatusCode::OK, self).into_response()
    }
}

async fn start_paxos_server(port: usize, other_ports: Vec<usize>) {}

struct MyExtractor {
    _method: Method,
    path: String,
    headers: HeaderMap,
    query: String,
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
            path,
            headers,
            query,
        })
    }
}

struct SharedState {}

// Why the heckie do I have this and the network struct?  Probably just working on it during different days
#[derive(Debug)]
struct SendToForwardServer {
    /// Keys are the acceptor_identifier while the values are the port which that acceptor is listening on
    acceptors: HashMap<usize, usize>,
}

const FORWARDING_PORT: &str = "3000";

#[async_trait]
impl basic_paxos_lib::SendToAcceptors for &SendToForwardServer {
    #[instrument]
    async fn send_accept(
        &self,
        acceptor_identifier: usize, // In this case this is the port which the acceprot is listening on
        value: usize,
        ballot_num: usize,
        proposer_identifier: usize,
    ) -> Result<AcceptedValue, HighestBallotPromised> {
        info!("sending_accept");
        let forwarding_port = FORWARDING_PORT;

        let path = "http://localhost:".to_string() + forwarding_port + "/accept";

        let request_body = AcceptRequestBody {
            propsing_ballot_num: ballot_num,
            node_identifier: proposer_identifier,
            value,
        };
        let request_body = serde_json::to_vec(&request_body).unwrap_or_else(|_| todo!());

        let request = hyper::Request::builder()
            .uri(path)
            .header(
                "forwarding-port",
                self.acceptors
                    .get(&acceptor_identifier)
                    .unwrap()
                    .to_string(),
            )
            .header("node-identifier", proposer_identifier)
            .header("content-type", "application/json")
            .method("POST")
            .body(Body::from(request_body))
            .unwrap();
        let (parts, body) = dbg!(request.into_parts());
        let request = Request::from_parts(parts, body);

        let client = hyper::Client::new();
        let response: hyper::Response<Body> = client.request(request).await.unwrap();
        dbg!(&response);

        // Currently the accept function only returns a result
        let (parts, body): (_, Body) = response.into_parts();
        dbg!("parts->{:?}", parts);
        let result = dbg!(
            serde_json::from_slice::<Result<AcceptedValue, HighestBallotPromised>>(
                &hyper::body::to_bytes(body).await.unwrap()
            )
            .unwrap()
        );
        result
    }

    #[instrument]
    async fn send_promise(
        &self,
        acceptor_identifier: usize,
        ballot_num: usize,
        proposer_identifier: usize,
    ) -> Result<Option<AcceptedValue>, basic_paxos_lib::PromiseReturn> {
        info!("sending promise");
        let forwarding_port = FORWARDING_PORT;

        let path = "http://localhost:".to_string() + forwarding_port + "/promise";

        let request_body = PromiseRequest {
            ballot_num,
            node_identifier: proposer_identifier,
        };
        let request_body = serde_json::to_vec(&request_body).unwrap_or_else(|_| todo!());

        let request = hyper::Request::builder()
            .uri(path)
            .header(
                "forwarding-port",
                self.acceptors
                    .get(&acceptor_identifier)
                    .unwrap()
                    .to_string(),
            )
            .header("node-identifier", proposer_identifier)
            .header("content-type", "application/json")
            .method("POST")
            .body(Body::from(request_body))
            .unwrap();
        let (parts, body) = dbg!(request.into_parts());
        let request = Request::from_parts(parts, body);

        let client = hyper::Client::new();
        println!("sending request to promise");
        let response: hyper::Response<Body> = dbg!(client.request(request).await.unwrap());
        println!("we got the promise response");

        // Currently the accept function only returns a result
        let (parts, body): (_, Body) = response.into_parts();
        let body = dbg!(hyper::body::to_bytes(body).await).unwrap_or_else(|_| todo!());

        serde_json::from_slice::<Result<Option<AcceptedValue>, PromiseReturn>>(dbg!(&body))
            .unwrap_or_else(|error| {
                dbg!(error);
                todo!()
            })
    }
}
