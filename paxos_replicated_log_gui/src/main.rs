use hyper::server;
use paxos_controllers::local_controller::{LocalMessageController};
use std::{
    collections::HashMap,
    sync::Arc,
};
use crate::back_end::{Server, GuiListenerResult, create_and_start_backend};
use tracing::Level;

use basic_paxos_lib::proposers::Proposer;
use clap::{command, value_parser, Arg, ArgAction};
use tokio::{
    sync::{Mutex, RwLock},
};

use crate::gui::run_gui;

mod Frames;
mod back_end;
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

    let server_count:usize = *args.get_one("ServerCount").unwrap();
    let (receive_frames, send_message_indices, propose_value_sender) = create_and_start_backend(server_count);

    run_gui(
        receive_frames,
        send_message_indices,
        server_count,
        propose_value_sender,
    );

    Ok(())
}

