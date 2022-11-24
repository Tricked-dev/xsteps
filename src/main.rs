#![allow(unused, clippy::single_match)]

use std::{
    io::{self, BufRead},
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
    thread,
};

use eframe::egui::{self, text};
use egui_extras::RetainedImage;
use futures::executor::block_on;
use once_cell::sync::OnceCell;
use parking_lot::Mutex;
use rdev::{listen, Button, Event, EventType, Key};
use screenshots::Screen;
use tokio::{
    runtime::Handle,
    sync::{
        mpsc::{self, Receiver},
        oneshot,
    },
    task::JoinHandle,
};

#[derive(Default)]
struct MyApp {
    worker: Option<JoinHandle<()>>,
    actions: Arc<Mutex<Vec<Actions>>>,
    done: bool,
}
#[derive(Default)]
struct CompletedApp {}

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
                        output.push_str(&format!("Press <key>\n"))
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
                        output.push_str(&format!("Press <key>\n"))
                    }
                    output.push_str("</body></html>");
                    std::fs::write("hello.html", output).unwrap();
                    std::process::exit(0);
                }
                egui::ScrollArea::vertical()
                    .always_show_scroll(true)
                    .stick_to_right(true)
                    .show(ui, |ui| {
                        frame.set_decorations(true);
                        frame.set_fullscreen(true);
                        for action in self.actions.lock().iter_mut() {
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
                    let mut result = Arc::clone(&self.actions);
                    let handle = tokio::spawn(async move {
                        let screens = Screen::all().unwrap();

                        let mut id = 0;

                        let mut rx = create_event_listener();
                        while let Some(i) = rx.recv().await {
                            match i.event_type {
                                EventType::ButtonPress(button) => {
                                    if button == Button::Left {
                                        for screen in &screens {
                                            println!("capturer {:?}", screen);
                                            let c = *screen;
                                            let clone = Arc::clone(&result);
                                            thread::spawn(move || {
                                                let mut image = c.capture().unwrap();
                                                let mut buffer = image.buffer();
                                                let bytes = buffer.to_vec();
                                                clone.lock().push(Actions {
                                                    image: bytes.clone(),
                                                    comment: String::new(),
                                                    size: (image.width(), image.height()),
                                                    texture: Some(
                                                        RetainedImage::from_image_bytes(
                                                            "screenshot.png",
                                                            &bytes,
                                                        )
                                                        .unwrap(),
                                                    ),
                                                });
                                                println!("Captured and saved screenshot");
                                            });
                                        }
                                    }
                                }

                                _ => {}
                            }
                            println!("id {id:0>10} got = {:?}", i);
                            id += 1;
                        }
                    });

                    self.worker = Some(handle);
                }
                if self.worker.is_some() && ui.button("stop").clicked() {
                    self.worker.take().unwrap().abort();
                    self.done = true;
                }
            }
        });
    }
}
struct Actions {
    pub image: Vec<u8>,
    pub comment: String,
    pub size: (u32, u32),
    pub texture: Option<RetainedImage>,
}
/// forgive me for this
pub fn create_event_listener() -> Receiver<Event> {
    let (tx, mut rx) = mpsc::channel::<Event>(32);
    tokio::spawn(async move {
        listen(move |event| {
            let tx = tx.clone();
            tokio::spawn(async move {
                tx.send(event).await;
            });
        })
    });
    rx
}

#[tokio::main]
async fn main() {
    let options = eframe::NativeOptions::default();
    eframe::run_native(
        "Xsteps",
        options,
        Box::new(|_cc| Box::new(MyApp::default())),
    );
}
