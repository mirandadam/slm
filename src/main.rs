mod gpu;
mod history;
mod reading;
mod sampler;
mod system;
mod text;
mod ui;

use std::sync::atomic::AtomicU64;
use std::sync::{Arc, Mutex};

use eframe::egui;

fn main() -> eframe::Result {
    let history = Arc::new(Mutex::new(history::History::default()));
    let interval = Arc::new(AtomicU64::new(ui::DEFAULT_REFRESH));
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            // The app id names the folder eframe saves the settings in (D14).
            .with_app_id("slm")
            .with_title(text::WINDOW_TITLE)
            // Its height then follows what it shows, and a person does not
            // resize it; eframe keeps only its position (D49).
            .with_inner_size(ui::WINDOW_SIZE)
            .with_resizable(false),
        ..Default::default()
    };
    eframe::run_native(
        "slm",
        options,
        Box::new(move |cc| {
            // The app first reads the saved refresh interval, so the thread
            // starts with it.
            let app = ui::App::new(cc.storage, history.clone(), interval.clone());
            let ctx = cc.egui_ctx.clone();
            sampler::spawn(interval, history, move || ctx.request_repaint());
            Ok(Box::new(app))
        }),
    )
}
