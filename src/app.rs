use std::cell::RefCell;
use std::rc::Rc;

use dioxus::desktop::tao::event::{Event as TaoEvent, WindowEvent};
use dioxus::desktop::{use_window, use_wry_event_handler};
use dioxus::prelude::*;
use futures_channel::mpsc::UnboundedReceiver;
use futures_util::StreamExt;

use crate::browsers::{self, Browser, Choice};
use crate::config::{Action, ConfigStore, Target};
use crate::macos::{self, Event};
use crate::router::{self, Request};
use crate::ui::picker::{self, Picker, PickerRequest};
use crate::ui::settings::{self, Settings};

pub struct Core {
    pub store: ConfigStore,
    pub browsers: Vec<Browser>,
    pub all_choices: Vec<Choice>,
    own_bundle_id: String,
}

impl Core {
    pub fn new(store: ConfigStore, own_bundle_id: String) -> Self {
        let mut core = Self {
            store,
            browsers: Vec::new(),
            all_choices: Vec::new(),
            own_bundle_id,
        };
        core.rescan_browsers();
        core
    }

    pub fn rescan_browsers(&mut self) {
        self.browsers = browsers::discover(&self.own_bundle_id);
        self.all_choices = browsers::all_choices(&self.browsers);
    }

    fn picker_choices(&self) -> Vec<Choice> {
        browsers::picker_choices(&self.browsers, self.store.config().picker_items.as_deref())
    }

    pub fn open(&self, target: &Target, url: &str) -> Result<(), String> {
        let Target { bundle_id, profile } = target;
        if *bundle_id == self.own_bundle_id {
            return Err("自分自身は指定できません".into());
        }
        let app_path = self
            .browsers
            .iter()
            .find(|b| &b.bundle_id == bundle_id)
            .map(|b| b.app_path.clone())
            .or_else(|| macos::app_path_for_bundle_id(bundle_id))
            .ok_or_else(|| format!("{bundle_id} が見つかりません"))?;
        macos::open_url(&app_path, bundle_id, url, profile.as_deref())
    }
}

/// AppleEvent はイベントループ開始前から届くので、main で作った受信側をここに預けておく
pub type Events = Rc<RefCell<Option<UnboundedReceiver<Event>>>>;

#[derive(Clone, PartialEq)]
enum View {
    Hidden,
    Picker(PickerRequest),
    Settings,
}

#[component]
pub fn App() -> Element {
    let core = use_context_provider(|| {
        Signal::new(Core::new(ConfigStore::load(), macos::own_bundle_id()))
    });
    let mut view = use_signal(|| initial_view(&core.peek()));

    use_hook(|| {
        let mut events = consume_context::<Events>().take().expect("events");
        spawn(async move {
            while let Some(event) = events.next().await {
                handle_event(core, view, event);
            }
        });
    });

    let desktop = use_window();
    use_effect(move || {
        let window = macos::ns_window(&desktop.window);
        match &*view.read() {
            View::Picker(request) => {
                macos::show_picker(&window, picker::window_size(request.choices.len()));
                // 前回表示したときにフォーカスが外れているとキー入力を受け取れない
                _ = desktop.webview.focus();
            }
            View::Settings => macos::show_settings(&window, settings::SIZE, settings::MIN_SIZE),
            View::Hidden => macos::hide(&window),
        }
        #[cfg(debug_assertions)]
        if std::env::var_os("LIGHTWELL_PREVIEW").is_some() {
            eprintln!("LIGHTWELL_WINDOW={}", window.windowNumber());
        }
    });

    // ウィンドウを閉じてもアプリは常駐させ続ける
    use_wry_event_handler(move |event, _| {
        let TaoEvent::WindowEvent { event, .. } = event else {
            return;
        };
        match event {
            WindowEvent::CloseRequested => view.set(View::Hidden),
            WindowEvent::Focused(false) if matches!(*view.peek(), View::Picker(_)) => {
                view.set(View::Hidden)
            }
            _ => {}
        }
    });

    match view() {
        View::Hidden => rsx! {},
        View::Settings => rsx! {
            Settings {}
        },
        View::Picker(request) => rsx! {
            Picker {
                request: request.clone(),
                on_done: move |n: Option<usize>| {
                    view.set(View::Hidden);
                    if let Some(choice) = n.and_then(|n| request.choices.get(n))
                        && let Err(e) = core.read().open(&choice.target, &request.url)
                    {
                        eprintln!("failed to open {}: {e}", request.url);
                    }
                },
            }
        },
    }
}

fn handle_event(mut core: Signal<Core>, mut view: Signal<View>, event: Event) {
    match event {
        Event::OpenUrl { url, sender_pid } => {
            let req = Request {
                url,
                source_app: sender_pid.and_then(macos::bundle_id_of_pid),
                fn_pressed: macos::fn_pressed(),
            };
            core.write().store.reload_if_changed();
            let core = core.read();
            if let Action::Browser(target) = router::route(core.store.config(), &req) {
                match core.open(target, &req.url) {
                    Ok(()) => return,
                    Err(e) => eprintln!("failed to open {}: {e}", req.url),
                }
            }
            view.set(View::Picker(PickerRequest {
                url: req.url,
                choices: core.picker_choices(),
            }));
        }
        Event::OpenSettings => {
            reload(core);
            view.set(View::Settings);
        }
        Event::ReloadConfig => reload(core),
        Event::SetDefaultBrowser => macos::set_as_default_browser(),
    }
}

fn reload(mut core: Signal<Core>) {
    let mut core = core.write();
    core.store.reload();
    core.rescan_browsers();
}

fn initial_view(core: &Core) -> View {
    #[cfg(debug_assertions)]
    if let Ok(preview) = std::env::var("LIGHTWELL_PREVIEW") {
        if std::env::var("LIGHTWELL_THEME").as_deref() == Ok("light") {
            macos::force_light_appearance();
        }
        if preview == "picker" {
            return View::Picker(PickerRequest {
                url: "https://github.com/shun-shobon/lightwell/pulls?q=is%3Aopen+is%3Apr".into(),
                choices: core.picker_choices(),
            });
        }
        return View::Settings;
    }
    if core.store.created() {
        View::Settings
    } else {
        View::Hidden
    }
}
