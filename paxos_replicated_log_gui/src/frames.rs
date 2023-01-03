use std::time::Duration;

use flume::Receiver;
use paxos_controllers::local_controller::Messages;
use tokio::time::timeout;

use crate::back_end::Server;

pub struct ServerFrame {
    pub prop_debug: String,
    pub acceptor_debug: String,
    pub decided_values: Vec<usize>,
}

pub struct Frame {
    pub servers: Vec<ServerFrame>,
    pub waiting_messages: Vec<Messages>,
}

/// Helper function used to create the [`ServerFrame`] structs for the current Frame
///
/// received_decided_values is how the proposer broadcasts that it decided a value.
pub(crate) async fn create_server_frames(
    servers: &[Server],
    receive_decided_values: &Receiver<(usize, usize)>,
) -> Vec<ServerFrame> {
    let mut server_frames: Vec<ServerFrame> = Vec::new();

    crate::back_end::update_decided_values(servers, receive_decided_values).await;

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
