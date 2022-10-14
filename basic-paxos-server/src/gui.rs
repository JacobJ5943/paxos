use std::{sync::Arc, thread};

use flume::{Receiver, Sender};
use tokio::sync::Mutex;

pub fn run_gui(
    runtime: Arc<tokio::runtime::Runtime>,
    local_proposer: Arc<Mutex<basic_paxos_lib::proposers::Proposer>>,
    local_acceptor: Arc<Mutex<basic_paxos_lib::acceptors::Acceptor>>,
) {
    println!("this went thorugh right?");

    let (sender, reciever) = flume::bounded::<u8>(1);
    let (acceptor_sendor, acceptor_receiver) = flume::bounded::<String>(1);
    let (proposer_sendor, proposer_receiver) = flume::bounded::<String>(1);

    let native_options = eframe::NativeOptions::default();
    thread::spawn(move || loop {
        reciever.recv();
        proposer_sendor.send(format!("{:?}", runtime.block_on(local_proposer.lock())));
        acceptor_sendor.send(format!("{:?}", runtime.block_on(local_acceptor.lock())));
    });

    eframe::run_native(
        "Visualizer",
        native_options,
        Box::new(|cc| {
            Box::new(MyEguiApp::new(
                cc,
                proposer_receiver,
                acceptor_receiver,
                sender,
            ))
        }),
    );
}

//// EFRAME THINGS BELOW
struct MyEguiApp {
    local_proposer: Receiver<String>,
    local_acceptor: Receiver<String>,
    updator: Sender<u8>,
}

impl MyEguiApp {
    fn new(
        _cc: &eframe::CreationContext<'_>,
        local_proposer: Receiver<String>,
        local_acceptor: Receiver<String>,
        updator: Sender<u8>,
    ) -> Self {
        Self {
            local_proposer,
            local_acceptor,
            updator,
        }
    }
}

impl eframe::App for MyEguiApp {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            egui::Grid::new("test_id").show(ui, |ui| {
                /*
                for (index, value) in self.current_waiting_list.iter().enumerate() {
                    if ui.button(format!("{}", value.0)).clicked() {
                        remove_vec.push(index)
                    }
                    ui.end_row()
                }
                */
                self.updator.send(0);
                ui.button(self.local_proposer.recv().unwrap());
                //ui.text_edit_multiline();
                ui.end_row();
                ui.button(self.local_acceptor.recv().unwrap());
                ui.end_row();

                let local_producer = self.local_proposer.clone();

                ui.end_row();
            });
        });
    }
}
