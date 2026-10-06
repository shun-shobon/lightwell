mod app;
mod browsers;
mod config;
mod macos;
mod router;
mod ui;

use std::cell::RefCell;
use std::rc::Rc;

use eframe::egui;
use objc2::MainThreadMarker;
use winit::platform::macos::{ActivationPolicy, EventLoopBuilderExtMacOS};

use crate::app::{App, Core};
use crate::config::ConfigStore;

fn main() -> eframe::Result {
    let mtm = MainThreadMarker::new().expect("main thread");
    macos::disable_app_nap();

    let core = Rc::new(RefCell::new(Core::new(
        ConfigStore::load(),
        macos::own_bundle_id(),
    )));
    let handler = {
        let core = core.clone();
        macos::Handler::new(mtm, move |event| app::handle_event(&core, event))
    };
    // 起動のきっかけになったリンクのイベントを取りこぼさないよう、イベントループ開始前に登録する
    macos::install_apple_event_handlers(&handler);

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Lightwell")
            .with_inner_size([360.0, 240.0])
            .with_visible(false)
            .with_transparent(true),
        // winitはデフォルトでRegularを明示的に設定するため、Info.plistのLSUIElementだけではDockに出てしまう
        event_loop_builder: Some(Box::new(|builder| {
            builder.with_activation_policy(ActivationPolicy::Accessory);
        })),
        ..Default::default()
    };
    eframe::run_native(
        "Lightwell",
        options,
        Box::new(move |cc| Ok(Box::new(App::new(cc, core, handler)))),
    )
}
