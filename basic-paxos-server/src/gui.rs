use std::{sync::Arc, thread};

use flume::{Receiver, Sender};
use tokio::sync::Mutex;

pub fn run_gui(
    runtime: Arc<tokio::runtime::Runtime>,
    local_proposer: Arc<Mutex<basic_paxos_lib::proposers::Proposer>>,
    local_acceptor: Arc<Mutex<basic_paxos_lib::acceptors::Acceptor>>,
) {
    let (sender, receiver) = flume::bounded::<u8>(1);
    let (acceptor_sender, acceptor_receiver) = flume::bounded::<String>(1);
    let (proposer_sender, proposer_receiver) = flume::bounded::<String>(1);

    let native_options = eframe::NativeOptions::default();
    thread::spawn(move || -> ! {
        let mut previous_proposer = "".to_string();
        let mut previous_acceptor = "".to_string();
        loop {
            receiver.recv().unwrap();
            let prop_sender_message = runtime.block_on(async {
                tokio::time::timeout(
                    std::time::Duration::from_millis(1000),
                    local_proposer.lock(),
                )
                .await
                .map(|lock_guard| format!("{:?}", lock_guard))
            });
            if let Ok(label) = prop_sender_message {
                proposer_sender
                    .send(format!("{:?}", label.clone()))
                    .unwrap();
                previous_proposer = label;
            } else {
                proposer_sender
                    .send(format!("{:?} -- Failed to update", previous_proposer))
                    .unwrap();
            }

            let acceptor_sender_message = runtime.block_on(async {
                tokio::time::timeout(
                    std::time::Duration::from_millis(1000),
                    local_acceptor.lock(),
                )
                .await
                .map(|lock_guard| format!("{:?}", lock_guard))
            });
            if let Ok(label) = acceptor_sender_message {
                acceptor_sender
                    .send(format!("{:?}", label.clone()))
                    .unwrap();
                previous_acceptor = label;
            } else {
                acceptor_sender
                    .send(format!("{:?} -- Failed to update", previous_acceptor))
                    .unwrap();
            }
        }
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
    updater: Sender<u8>,
}

impl MyEguiApp {
    fn new(
        _cc: &eframe::CreationContext<'_>,
        local_proposer: Receiver<String>,
        local_acceptor: Receiver<String>,
        updater: Sender<u8>,
    ) -> Self {
        Self {
            local_proposer,
            local_acceptor,
            updater,
        }
    }
}

impl eframe::App for MyEguiApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            egui::Grid::new("test_id").show(ui, |ui| {
                self.updater.send(0).unwrap();
                let _some_response = ui.button(self.local_proposer.recv().unwrap());
                ui.end_row();
                let _some_response = ui.button(self.local_acceptor.recv().unwrap());
                ui.end_row();
            });
        });
    }
}
