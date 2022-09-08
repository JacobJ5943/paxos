use eframe::egui;

fn main() {
    let native_options = eframe::NativeOptions::default();
    eframe::run_native("My egui App", native_options, Box::new(|cc| Box::new(MyEguiApp::new(cc))));
}

#[derive(Default)]
struct MyEguiApp {
    current_waiting_list:Vec<usize>
}

impl MyEguiApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        // Customize egui here with cc.egui_ctx.set_fonts and cc.egui_ctx.set_visuals.
        // Restore app state using cc.storage (requires the "persistence" feature).
        // Use the cc.gl (a glow::Context) to create graphics shaders and buffers that you can use
        // for e.g. egui::PaintCallback.
        Self {
            current_waiting_list:vec![1,2,3,4,5,6,7,8,9]
        }
    }
}

impl eframe::App for MyEguiApp {
   fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
       egui::CentralPanel::default().show(ctx, |ui| {

        egui::Grid::new("test_id").show(ui,|ui|{
            let mut remove_vec:Vec<usize> = Vec::new();
            for (index, value) in self.current_waiting_list.iter().enumerate() {
                if ui.button(format!("{}", value)).clicked() {
                    remove_vec.push(index)
                }
                ui.end_row()
            }

            for value in remove_vec.into_iter().rev() {
                self.current_waiting_list.remove(value);
            }
        });
       });

   }
}