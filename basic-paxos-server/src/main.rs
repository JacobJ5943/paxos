use std::{cell::RefCell, collections::HashMap, convert::Infallible, net::SocketAddr, sync::Arc, borrow::Borrow};

use async_trait::async_trait;
use axum::{
    extract,
    extract::{FromRequest, Json, RequestParts},
    http::{HeaderMap, Method, Uri},
    routing::post,
    Extension, Router,
};
use basic_paxos_lib::SendToAcceptors;
use hyper::Body;
use serde::Deserialize;
use tokio::sync::{Mutex, RwLock};

// 1.21.0 was having an issue with main for some unknown reason
#[tokio::ain(flavor = "multi_thread")]
async fn main() {
    println!("Hello, world!");
    let socket = 4333;
    let shared_state = Arc::new(Mutex::new(SharedState {}));

    let shared_acceptor = Arc::new(Mutex::new(basic_paxos_lib::acceptors::Acceptor::default()));

    let acceptor_network_vec: Vec<AcceptorNetworkStruct> = Vec::new();
    let acceptor_network_vec = Arc::new(RwLock::new(acceptor_network_vec));

    let sender_client = Arc::new(SendToForwardServer{ acceptors: HashMap::new() });
    let app = Router::new()
        .route("/accept", post(accept))
        .layer(Extension(shared_acceptor))
        .layer(Extension(shared_state))
        .route("/propose_value", post(propose_value))
        .layer(Extension(acceptor_network_vec))
        .layer(Extension(sender_client));

    let addr = SocketAddr::from(([127, 0, 0, 1], socket));

    axum::Server::bind(&addr)
        .serve(app.into_make_service())
        .await;
}

#[derive(Deserialize)]
struct AcceptRequestBody {
    propsing_ballot_num: usize,
    node_identifier: usize,
    value: usize,
}
/// This is what the acceptors will receive accept requests on
/// This will need to have access to the acceptors, but doesn't need access to anything else
/// TODO The return type should probably not be that
async fn accept(
    acceptor_state: Extension<Arc<Mutex<basic_paxos_lib::acceptors::Acceptor>>>,
    Json(request_body): extract::Json<AcceptRequestBody>,
) -> Result<(), ()> {
    let mut lock = acceptor_state.lock().await;
    lock.accept(
        request_body.propsing_ballot_num,
        request_body.node_identifier,
        request_body.value,
    )
}

#[derive(Deserialize, Debug)]
struct ProposeValueResut {
    proposing_value: usize,
}

#[derive(Debug)]
struct AcceptorNetworkStruct {}

/// The endpoint which the clients will call to propose a value.
/// This will need access to proposers and possibly it's acceptor.  
/// The reason for this is instead of calling accept to this node's port
/// it could just call the accept function
async fn propose_value(
    Json(proposing_value): Json<ProposeValueResut>,
    Extension(acceptor_network_vec): Extension<Arc<RwLock<Vec<AcceptorNetworkStruct>>>>,
    Extension(local_proposer):Extension<Arc<Mutex<basic_paxos_lib::proposers::Proposer>>>,
    Extension(send_to_forward_server):Extension<Arc<SendToForwardServer>>
    // I think it's fine for this to lock the whole proposer since all proposed values should be sequential for the node
    // I don't want this function to return until a value has been decided though so the single threaded thing wouldn't be a benefit necisarilyQ
) -> Result<(),()>{
    let mut lock = local_proposer.lock().await;
    let current_highest_ballot = lock.current_highest_ballot;
    match lock.propose_value(
        current_highest_ballot,
        &send_to_forward_server.borrow(),
        &mut vec![1,2,3],
        3
    ).await {
        Ok(_) => Ok(()),
        Err(_) => Err(())
    }
}

struct PromiseRequest {
    ballot_num: usize,
    node_identifier: usize,
}

/// The endpoint which acceptors will receive promises on
async fn receive_promise() {}

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

struct SendToForwardServer {
    /// Keys are the acceptor_identifier while the values are the port which that acceptor is listening on
    acceptors: HashMap<usize, usize>,
}

const FORWARDING_PORT: &str = "3000";

#[async_trait]
impl basic_paxos_lib::SendToAcceptors for &SendToForwardServer {
    async fn send_accept(
        &self,
        acceptor_identifier: usize,
        value: usize,
        ballot_num: usize,
        proposer_identifier: usize,
    ) -> Result<(), ()> {
        let forwarding_port = FORWARDING_PORT;

        let path = "http://localhost:".to_string() + forwarding_port + "/accept";

        let request = hyper::Request::builder()
            .uri(path)
            .header(
                "forwarding-port",
                self.acceptors
                    .get(&acceptor_identifier)
                    .unwrap()
                    .to_string(),
            )
            .header("content-type", "application/json")
            .method("POST")
            .body(Body::from(""))
            .unwrap();
        let client = hyper::Client::new();
        let response = client.request(request).await.unwrap();

        todo!()
    }
    async fn send_promise(
        &self,
        acceptor_identifier: usize,
        ballot_num: usize,
        proposer_identifier: usize,
    ) -> Result<(), basic_paxos_lib::PromiseReturn> {
        todo!()
    }
}

