#![allow(clippy::single_match)]

use std::{io::Cursor, sync::Arc, thread};

use eframe::egui;
use egui_extras::RetainedImage;
use image::DynamicImage::ImageRgba8;
use parking_lot::Mutex;
use rdev::{listen, Button, Event, EventType, Key};
use screenshots::Screen;
use tokio::{
    sync::mpsc::{self, Receiver},
    task::JoinHandle,
};
use tracing::{debug, info};

#[derive(Default)]
struct MyApp {
    worker: Option<JoinHandle<()>>,
    workers: Arc<Mutex<Vec<std::thread::JoinHandle<()>>>>,
    actions: Arc<Mutex<Vec<Actions>>>,
    comment: String,
    done: bool,
}

impl eframe::App for MyApp {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            if self.done {
                ui.heading("Xsteps is done!");
                if ui.button("exit").clicked() {
                    std::process::exit(0);
                }
                if ui.button("save as md").clicked() {
                    let result = self.actions.lock();
                    let mut output = String::new();
                    for action in result.iter() {
                        output.push_str(&format!(
                            "![](data:image/png;base64,{})\n",
                            base64::encode(&action.image)
                        ));
                        output.push_str("Press <key>\n")
                    }
                    std::fs::write("hello.md", output).unwrap();
                    std::process::exit(0);
                }
                if ui.button("save as html").clicked() {
                    let result = self.actions.lock();
                    let mut output = String::new();
                    output.push_str("<html><body>");
                    for action in result.iter() {
                        output.push_str(&format!(
                            "<img src=\"data:image/png;base64,{}\"/>",
                            base64::encode(&action.image)
                        ));
                        output.push_str("Press <key>\n")
                    }
                    output.push_str("</body></html>");
                    std::fs::write("hello.html", output).unwrap();
                    std::process::exit(0);
                }
                if ui.button("write as png").clicked() {
                    let result = self.actions.lock();
                    for (i, action) in result.iter().enumerate() {
                        std::fs::write(format!("{i:0>3}.png"), &action.image).unwrap();
                    }
                    std::process::exit(0);
                }
                egui::ScrollArea::vertical()
                    .always_show_scroll(true)
                    .stick_to_right(true)
                    .show(ui, |ui| {
                        frame.set_decorations(true);
                        frame.set_fullscreen(true);
                        for action in self.actions.lock().iter_mut() {
                            ui.heading(&action.comment);
                            let img = action.texture.get_or_insert_with(|| {
                                // Load the texture only once.
                                RetainedImage::from_image_bytes(
                                    "screenshot.png",
                                    action.image.as_slice(),
                                )
                                .unwrap()
                            });
                            let width = ui.painter().clip_rect().width() * 0.7;
                            let height = width * (action.size.1 as f32 / action.size.0 as f32);
                            ui.image(img.texture_id(ctx), egui::Vec2::new(width, height));
                        }
                    });
            } else {
                ui.heading("Xsteps");
                if self.worker.is_none() && ui.button("start").clicked() {
                    let result = Arc::clone(&self.actions);
                    let workers = Arc::clone(&self.workers);
                    let handle = tokio::spawn(async move {
                        let screens = Screen::all().unwrap();

                        let mut rx = create_event_listener();
                        let mut mouse_loc = (0f64, 0f64);
                        while let Some(i) = rx.recv().await {
                            match i.event_type {
                                EventType::MouseMove { x, y } => {
                                    mouse_loc.0 = x;
                                    mouse_loc.1 = y;
                                }
                                EventType::ButtonPress(Button::Left) => {
                                    let screen = screens
                                        .iter()
                                        .find(|s| {
                                            let x = s.display_info.x as f64;
                                            let y = s.display_info.y as f64;
                                            let w = s.display_info.width as f64;
                                            let h = s.display_info.height as f64;
                                            mouse_loc.0 >= x
                                                && mouse_loc.0 <= x + w
                                                && mouse_loc.1 >= y
                                                && mouse_loc.1 <= y + h
                                        })
                                        .unwrap_or(&screens[0]);

                                    debug!("capturer {:?}", screen);
                                    let c = *screen;
                                    let clone = Arc::clone(&result);
                                    let handle = thread::spawn(move || {
                                        let image = c.capture().unwrap();
                                        let buffer = image.buffer();
                                        let now = std::time::Instant::now();
                                        let image = image::load_from_memory(buffer)
                                            .map_err(|err| err.to_string())
                                            .unwrap();

                                        // write a yellow circle around the cursor on  "image"
                                        let mut image = image.to_rgba8();
                                        let (x, y) = mouse_loc;
                                        let radius = 400f64;
                                        let x = (x - c.display_info.x as f64) - radius / 2.0;
                                        let y = (y - c.display_info.y as f64) - radius / 2.0;
                                        for i in 0..radius as u32 {
                                            for j in 0..radius as u32 {
                                                if (i as f64 - radius as f64 / 2.0).powi(2)
                                                    + (j as f64 - radius as f64 / 2.0).powi(2)
                                                    < radius as f64 / 2.0
                                                {
                                                    image.put_pixel(
                                                        x as u32 + i,
                                                        y as u32 + j,
                                                        image::Rgba([255, 255, 0, 40]),
                                                    );
                                                }
                                            }
                                        }

                                        let image = ImageRgba8(image);

                                        let mut buffer = Vec::new();
                                        let mut cur = Cursor::new(&mut buffer);
                                        image
                                            .write_to(&mut cur, image::ImageOutputFormat::Png)
                                            .unwrap();

                                        let size = [image.width() as _, image.height() as _];
                                        let image_buffer = image.to_rgba8();
                                        let pixels = image_buffer.as_flat_samples();

                                        let loaded = egui::ColorImage::from_rgba_unmultiplied(
                                            size,
                                            pixels.as_slice(),
                                        );

                                        info!("took {:?} to translate image", now.elapsed());
                                        clone.lock().push(Actions {
                                            image: buffer,
                                            comment: String::new(),
                                            size: (image.width(), image.height()),
                                            texture: Some(RetainedImage::from_color_image(
                                                "screenshot.png",
                                                loaded,
                                            )),
                                        });
                                        debug!("Captured and saved screenshot");
                                    });
                                    workers.lock().push(handle);
                                }
                                EventType::KeyPress(Key::Return) => {
                                    // std::process::exit(0);
                                }

                                _ => {}
                            }
                        }
                    });
                    self.worker = Some(handle);
                }
                if self.worker.is_some() {
                    if ui.text_edit_singleline(&mut self.comment).lost_focus() {
                        if self.actions.lock().len() != 0 {
                            self.actions.lock().last_mut().unwrap().comment = self.comment.clone();
                        }
                        self.comment = String::new();
                    }

                    if ui.button("stop").clicked() {
                        debug!("exiting the event stream");
                        self.worker.take().unwrap().abort();
                        for worker in self.workers.lock().drain(..) {
                            debug!("completing a screenshot");
                            worker.join().unwrap();
                        }

                        debug!("removing the last screenshot");
                        self.actions.lock().pop();
                        info!("done");
                        self.done = true;
                    }
                }
            }
        });
    }
}
struct Actions {
    pub image: Vec<u8>,
    #[allow(dead_code)]
    pub comment: String,
    pub size: (u32, u32),
    pub texture: Option<RetainedImage>,
}
/// forgive me for this
pub fn create_event_listener() -> Receiver<Event> {
    let (tx, rx) = mpsc::channel::<Event>(32);
    tokio::spawn(async move {
        listen(move |event| {
            let tx = tx.clone();
            tokio::spawn(async move { tx.send(event).await.ok() });
        })
    });
    rx
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();
    let options = eframe::NativeOptions::default();
    eframe::run_native(
        "Xsteps",
        options,
        Box::new(|_cc| Box::new(MyApp::default())),
    );
}
