use std::collections::{HashSet, VecDeque};

use egui::{Align2, Color32, Stroke};
use flume::{Receiver, Sender};
use paxos_controllers::local_controller::Messages;

use crate::Frame;

pub fn run_gui(
    receive_frames: Receiver<Frame>,
    send_message_indecies: Sender<Vec<Messages>>,
    server_count: usize,
    propose_value_sender: Sender<(usize, usize)>,
) {
    let native_options = eframe::NativeOptions::default();
    eframe::run_native(
        "Visualizer",
        native_options,
        Box::new(move |cc| {
            Box::new(MyEguiApp::new(
                cc,
                receive_frames,
                send_message_indecies,
                server_count,
                propose_value_sender,
            ))
        }),
    );
}

//// EFRAME THINGS BELOW
struct MyEguiApp {
    receive_frames: Receiver<Frame>,
    send_message_indecies: Sender<Vec<Messages>>,
    propose_value_buffers: Vec<String>,
    propose_value_sender: Sender<(usize, usize)>,
    waiting_proposed_values: VecDeque<(usize, usize)>,
}

impl MyEguiApp {
    fn new(
        _cc: &eframe::CreationContext<'_>,
        receive_frames: Receiver<Frame>,
        send_message_indecies: Sender<Vec<Messages>>,
        server_count: usize,
        propose_value_sender: Sender<(usize, usize)>,
    ) -> Self {
        Self {
            receive_frames,
            send_message_indecies,
            propose_value_buffers: vec!["".to_string(); server_count],
            propose_value_sender,
            waiting_proposed_values: VecDeque::new(),
        }
    }
}

impl eframe::App for MyEguiApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let full_window_size = ctx.used_size();
        egui::CentralPanel::default().show(ctx, |_ui| {
            let frame = self.receive_frames.recv().unwrap();
            let stroke_width = 2.0;

            egui::Window::new("Servers window")
                .anchor(Align2::CENTER_TOP, [0.0, 0.0])
                .scroll2([false, true])
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        for (index, server) in frame.servers.iter().enumerate() {
                            egui::Frame::none()
                                .stroke(Stroke {
                                    width: stroke_width,
                                    color: Color32::RED,
                                })
                                .show(ui, |ui| {
                                    ui.set_height(0.0);
                                    ui.set_width(
                                        (full_window_size.x / frame.servers.len() as f32)
                                            - (stroke_width * 2.0),
                                    );
                                    ui.centered_and_justified(|ui| {
                                        ui.text_edit_multiline(&mut server.acceptor_debug.clone());
                                        ui.text_edit_multiline(&mut server.prop_debug.clone());
                                        ui.label("Decided_values:");

                                        ui.text_edit_multiline(
                                            &mut server.decided_values.iter().enumerate().fold(
                                                String::new(),
                                                |acc, next| {
                                                    acc + "\n"
                                                        + &format!(
                                                            "Slot:{},Value:{}",
                                                            next.0, next.1
                                                        )
                                                },
                                            ),
                                        );

                                        ui.label("Propose Value");
                                        if ui
                                            .text_edit_singleline(
                                                self.propose_value_buffers.get_mut(index).unwrap(),
                                            )
                                            .changed()
                                        {
                                            // If i have a number 123456 and I try to insert a 'b' after the 3 it will not insert the 'b' and will move the cursor one space to the right
                                            *self.propose_value_buffers.get_mut(index).unwrap() =
                                                self.propose_value_buffers
                                                    .get(index)
                                                    .unwrap()
                                                    .chars()
                                                    .filter(|c| {
                                                        HashSet::from([
                                                            '0', '1', '2', '3', '4', '5', '6', '7',
                                                            '8', '9',
                                                        ])
                                                        .contains(c)
                                                    })
                                                    .collect();
                                        }

                                        if ui.button("propose_value").clicked()
                                            && !self
                                                .propose_value_buffers
                                                .get(index)
                                                .unwrap()
                                                .is_empty()
                                        {
                                            self.waiting_proposed_values.push_back((
                                                index,
                                                self.propose_value_buffers
                                                    .get(index)
                                                    .unwrap()
                                                    .parse::<usize>()
                                                    .unwrap(),
                                            ));
                                            println!("This would proposer the value")
                                        };
                                    });
                                });
                        } // End for loop
                    })
                });

            // Messages in flight frame

            let mut pushed_buttons = Vec::new();
            egui::Window::new("Messages in flight")
                .anchor(Align2::CENTER_BOTTOM, [0.0, 0.0])
                .collapsible(true)
                .scroll2([false, true])
                .show(ctx, |ui| {
                    ui.set_width(full_window_size.x - (stroke_width * 2.0));

                    for proposed_value in self.waiting_proposed_values.drain(..) {
                        dbg!(proposed_value);
                        self.propose_value_sender.send(proposed_value).unwrap();
                    }

                    ui.horizontal_wrapped(|ui| {
                        for (_index, message) in frame.waiting_messages.iter().enumerate() {
                            if ui.button(&format!("{:?}", message)).clicked() {
                                pushed_buttons.push(message.clone());
                            }
                        }
                    });
                });
            self.send_message_indecies.send(pushed_buttons).unwrap();
        });
    }
}
