mod app;
mod browsers;
mod config;
mod macos;
mod router;
mod ui;

use std::cell::RefCell;
use std::rc::Rc;

use dioxus::desktop::tao::event::{Event, StartCause};
use dioxus::desktop::tao::event_loop::EventLoopBuilder;
use dioxus::desktop::tao::platform::macos::{ActivationPolicy, EventLoopExtMacOS};
use dioxus::desktop::{Config, WindowBuilder, WindowCloseBehaviour};
use dioxus::prelude::*;
use objc2::MainThreadMarker;

use crate::app::{App, Events};

fn main() {
    let mtm = MainThreadMarker::new().expect("main thread");
    macos::disable_app_nap();

    let (tx, rx) = futures_channel::mpsc::unbounded();
    let handler = macos::Handler::new(mtm, move |event| _ = tx.unbounded_send(event));
    macos::install_apple_event_handlers_on_launch(&handler);

    // taoはデフォルトでRegularを明示的に設定するため、Info.plistのLSUIElementだけではDockに出てしまう
    let mut event_loop = EventLoopBuilder::with_user_event().build();
    event_loop.set_activation_policy(ActivationPolicy::Accessory);
    let status_item = RefCell::new(None);
    let config = Config::new()
        .with_event_loop(event_loop)
        .with_window(
            WindowBuilder::new()
                .with_title("Lightwell")
                .with_visible(false)
                .with_transparent(true),
        )
        .with_menu(None)
        // wryのファイルドロップ処理が有効だと、HTMLのドラッグ&ドロップでの並べ替えが効かない
        .with_disable_drag_drop_handler(true)
        .with_close_behaviour(WindowCloseBehaviour::WindowHides)
        .with_custom_head(format!("<style>{}</style>", ui::STYLE))
        .with_custom_event_handler(move |event, _| {
            if let Event::NewEvents(StartCause::Init) = event {
                status_item.replace(Some(macos::install_status_item(&handler, mtm)));
            }
        });

    let events: Events = Rc::new(RefCell::new(Some(rx)));
    let dom = VirtualDom::new(App).with_root_context(events);
    dioxus::desktop::launch::launch_virtual_dom(dom, config);
}
