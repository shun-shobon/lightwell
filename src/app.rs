use std::cell::RefCell;
use std::rc::Rc;

use eframe::egui;
use objc2::rc::Retained;
use objc2_app_kit::{NSStatusItem, NSWindow};

use crate::browsers::{self, Browser, Choice};
use crate::config::{Action, ConfigStore, Target};
use crate::macos::{self, Event, Handler};
use crate::router::{self, Request};
use crate::ui::picker::{self, PickerState};
use crate::ui::settings::{self, Settings};
use crate::ui::{Textures, theme};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Hidden,
    Picker,
    Settings,
}

pub struct Core {
    pub store: ConfigStore,
    pub browsers: Vec<Browser>,
    pub all_choices: Vec<Choice>,
    pub picker_choices: Vec<Choice>,
    pub browsers_rev: u64,
    pub pending: Option<Request>,
    pub mode: Mode,
    own_bundle_id: String,
    window: Option<Retained<NSWindow>>,
    ctx: Option<egui::Context>,
}

pub type SharedCore = Rc<RefCell<Core>>;

impl Core {
    pub fn new(store: ConfigStore, own_bundle_id: String) -> Self {
        let mut core = Self {
            store,
            browsers: Vec::new(),
            all_choices: Vec::new(),
            picker_choices: Vec::new(),
            browsers_rev: 0,
            pending: None,
            mode: Mode::Hidden,
            own_bundle_id,
            window: None,
            ctx: None,
        };
        core.rescan_browsers();
        core
    }

    pub fn rescan_browsers(&mut self) {
        self.browsers = browsers::discover(&self.own_bundle_id);
        self.all_choices = browsers::all_choices(&self.browsers);
        self.browsers_rev += 1;
    }

    fn refresh_picker_choices(&mut self) {
        self.picker_choices =
            browsers::picker_choices(&self.browsers, self.store.config().picker_items.as_deref());
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

    fn picker_size(&self) -> (f64, f64) {
        picker::window_size(self.picker_choices.len())
    }
}

/// AppKitのウィンドウ操作はwinit経由で同期的にeframeの描画 (= Core の借用) を引き起こしうるので、
/// Core の借用を手放してから行う
pub fn present(core: &SharedCore, mode: Mode) {
    let (window, ctx, picker_size) = {
        let mut c = core.borrow_mut();
        c.mode = mode;
        if mode == Mode::Picker {
            c.refresh_picker_choices();
        }
        (c.window.clone(), c.ctx.clone(), c.picker_size())
    };
    if let Some(window) = window {
        match mode {
            Mode::Picker => macos::show_picker(&window, picker_size),
            Mode::Settings => macos::show_settings(&window, settings::SIZE, settings::MIN_SIZE),
            Mode::Hidden => macos::hide(&window),
        }
    }
    if let Some(ctx) = ctx {
        ctx.request_repaint();
    }
}

pub fn handle_event(core: &SharedCore, event: Event) {
    match event {
        Event::OpenUrl { url, sender_pid } => {
            let opened = {
                let mut c = core.borrow_mut();
                c.store.reload_if_changed();
                let req = Request {
                    url,
                    source_app: sender_pid.and_then(macos::bundle_id_of_pid),
                    fn_pressed: macos::fn_pressed(),
                };
                let opened = match router::route(c.store.config(), &req) {
                    Action::Picker => false,
                    Action::Browser(target) => c
                        .open(target, &req.url)
                        .inspect_err(|e| eprintln!("failed to open {}: {e}", req.url))
                        .is_ok(),
                };
                if !opened {
                    c.pending = Some(req);
                }
                opened
            };
            if !opened {
                present(core, Mode::Picker);
            }
        }
        Event::OpenSettings => {
            {
                let mut c = core.borrow_mut();
                c.pending = None;
                c.store.reload();
                c.rescan_browsers();
            }
            present(core, Mode::Settings);
        }
        Event::ReloadConfig => {
            let mut c = core.borrow_mut();
            c.store.reload();
            c.rescan_browsers();
        }
        Event::SetDefaultBrowser => macos::set_as_default_browser(),
    }
}

pub struct App {
    core: SharedCore,
    _handler: Retained<Handler>,
    _status_item: Retained<NSStatusItem>,
    textures: Textures,
    picker: PickerState,
    settings: Option<Settings>,
}

impl App {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        core: SharedCore,
        handler: Retained<Handler>,
    ) -> Self {
        let mtm = objc2::MainThreadMarker::new().expect("main thread");
        macos::install_apple_event_handlers(&handler);
        let status_item = macos::install_status_item(&handler, mtm);
        theme::install(&cc.egui_ctx, macos::accent_color());

        let (pending_mode, first_launch) = {
            let mut c = core.borrow_mut();
            c.window = macos::ns_window(cc);
            if let Some(window) = &c.window {
                macos::conceal_initial_window(window);
            }
            c.ctx = Some(cc.egui_ctx.clone());
            (c.mode, c.store.created())
        };
        match pending_mode {
            // 起動のきっかけになったリンクが、ウィンドウ作成前に届いていた場合
            Mode::Picker | Mode::Settings => present(&core, pending_mode),
            Mode::Hidden if first_launch => present(&core, Mode::Settings),
            Mode::Hidden => {}
        }
        #[cfg(debug_assertions)]
        preview(&core);

        Self {
            core,
            _handler: handler,
            _status_item: status_item,
            textures: Textures::default(),
            picker: PickerState::default(),
            settings: None,
        }
    }

    fn picker_ui(&mut self, ui: &mut egui::Ui) {
        let outcome = {
            let core = self.core.borrow();
            let url = core.pending.as_ref().map_or("", |r| r.url.as_str());
            picker::show(
                ui,
                &mut self.picker,
                url,
                &core.picker_choices,
                &mut self.textures,
            )
        };
        let Some(outcome) = outcome else { return };
        let (pending, target) = {
            let mut c = self.core.borrow_mut();
            let target = match outcome {
                picker::Outcome::Choose(n) => c.picker_choices.get(n).map(|ch| ch.target.clone()),
                picker::Outcome::Cancel => None,
            };
            (c.pending.take(), target)
        };
        self.picker = PickerState::default();
        present(&self.core, Mode::Hidden);
        if let (Some(req), Some(target)) = (pending, target)
            && let Err(e) = self.core.borrow().open(&target, &req.url)
        {
            eprintln!("failed to open {}: {e}", req.url);
        }
    }

    fn settings_ui(&mut self, ui: &mut egui::Ui) {
        let settings = self
            .settings
            .get_or_insert_with(|| Settings::new(self.core.borrow().store.config()));
        let fx = {
            let core = self.core.borrow();
            settings.show(ui, &core, &mut self.textures)
        };
        if let Some(config) = fx.save {
            settings.error = self
                .core
                .borrow_mut()
                .store
                .save(config)
                .err()
                .map(|e| format!("保存に失敗しました: {e}"));
        }
        if fx.set_default_browser {
            macos::set_as_default_browser();
        }
        if let Some(enabled) = fx.login_item {
            if let Err(e) = macos::set_login_item(enabled) {
                settings.error = Some(format!("ログイン項目の変更に失敗しました: {e}"));
            }
            settings.login_item = macos::login_item_status();
        }
        // モーダルダイアログ中もイベントループが回るので、Core を借用していない所で出す
        if let Some(i) = fx.choose_source_app
            && let Some(mtm) = objc2::MainThreadMarker::new()
            && let Some(bundle_id) = macos::choose_app(mtm)
            && let Some(rule) = settings.draft.rules.get_mut(i)
            && !rule.source_apps.contains(&bundle_id)
        {
            rule.source_apps.push(bundle_id);
        }
    }
}

impl eframe::App for App {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        // 背面のすりガラス (NSVisualEffectView) を透かす
        [0.0; 4]
    }

    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // ウィンドウを閉じてもアプリは常駐させ続ける
        if ctx.input(|i| i.viewport().close_requested()) {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            present(&self.core, Mode::Hidden);
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let mode = {
            let core = self.core.borrow();
            self.textures
                .sync_browsers(ui.ctx(), &core.browsers, core.browsers_rev);
            core.mode
        };
        if mode != Mode::Settings {
            self.settings = None;
        }
        match mode {
            Mode::Picker => self.picker_ui(ui),
            Mode::Settings => self.settings_ui(ui),
            // eframeが最初の描画後に勝手に表示したウィンドウを隠す
            Mode::Hidden => {
                if let Some(window) = &self.core.borrow().window {
                    macos::order_out(window);
                }
                ui.ctx().request_repaint();
            }
        }
    }
}

#[cfg(debug_assertions)]
fn preview(core: &SharedCore) {
    let Ok(target) = std::env::var("LIGHTWELL_PREVIEW") else {
        return;
    };
    let mode = if target == "picker" {
        core.borrow_mut().pending = Some(Request {
            url: "https://github.com/shun-shobon/lightwell/pulls?q=is%3Aopen+is%3Apr".into(),
            ..Default::default()
        });
        Mode::Picker
    } else {
        Mode::Settings
    };
    if std::env::var("LIGHTWELL_THEME").as_deref() == Ok("light") {
        if let Some(ctx) = &core.borrow().ctx {
            ctx.set_theme(egui::Theme::Light);
        }
        macos::force_light_appearance();
    }
    present(core, mode);
    if let Some(window) = &core.borrow().window {
        eprintln!("LIGHTWELL_WINDOW={}", macos::window_number(window));
    }
}
