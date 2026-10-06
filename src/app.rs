use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use eframe::egui::{self, Color32, Key, RichText, TextureHandle};
use objc2::rc::Retained;
use objc2_app_kit::{NSStatusItem, NSWindow};

use crate::browsers::{self, Browser, Choice, ICON_SIZE};
use crate::config::{Action, Config, ConfigStore, Rule, Target};
use crate::macos::{self, Event, Handler, LoginItemStatus};
use crate::router::{self, Request};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Hidden,
    Picker,
    Settings,
}

const PICKER_WIDTH: f64 = 360.0;
const PICKER_ROW_HEIGHT: f64 = 36.0;
const PICKER_CHROME_HEIGHT: f64 = 64.0;
const SETTINGS_SIZE: (f64, f64) = (760.0, 640.0);

pub struct Core {
    pub store: ConfigStore,
    pub browsers: Vec<Browser>,
    /// 検出したすべてのブラウザ・プロファイル (設定画面のコンボ用)
    pub all_choices: Vec<Choice>,
    /// 選択UIに並べる項目 (設定の picker_items を反映)
    pub picker_choices: Vec<Choice>,
    /// ブラウザ一覧を再取得した回数。アイコンのテクスチャを作り直す判定に使う
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
            browsers::picker_choices(&self.browsers, &self.store.config().picker_items);
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
        let rows = self.picker_choices.len().max(1) as f64;
        (
            PICKER_WIDTH,
            PICKER_CHROME_HEIGHT + rows * PICKER_ROW_HEIGHT,
        )
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
            Mode::Settings => macos::show_settings(&window, SETTINGS_SIZE),
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
    icons: HashMap<String, TextureHandle>,
    icons_rev: u64,
    picker: PickerState,
    settings: Option<SettingsState>,
}

#[derive(Default)]
struct PickerState {
    selected: usize,
    seen_focus: bool,
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
        install_japanese_font(&cc.egui_ctx);

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

        Self {
            core,
            _handler: handler,
            _status_item: status_item,
            icons: HashMap::new(),
            icons_rev: 0,
            picker: PickerState::default(),
            settings: None,
        }
    }

    fn sync_icons(&mut self, ctx: &egui::Context) {
        let core = self.core.borrow();
        if self.icons_rev == core.browsers_rev {
            return;
        }
        self.icons_rev = core.browsers_rev;
        self.icons = core
            .browsers
            .iter()
            .filter_map(|b| {
                let rgba = b.icon.as_ref()?;
                let image = egui::ColorImage::from_rgba_premultiplied([ICON_SIZE, ICON_SIZE], rgba);
                let texture = ctx.load_texture(&b.bundle_id, image, egui::TextureOptions::LINEAR);
                Some((b.bundle_id.clone(), texture))
            })
            .collect();
    }

    fn picker_ui(&mut self, ui: &mut egui::Ui) {
        enum Outcome {
            Choose(usize),
            Cancel,
        }
        let mut outcome = None;
        {
            let core = self.core.borrow();
            let count = core.picker_choices.len();
            let url = core.pending.as_ref().map_or("", |r| r.url.as_str());

            ui.input(|i| {
                if i.key_pressed(Key::Escape) {
                    outcome = Some(Outcome::Cancel);
                }
                if i.key_pressed(Key::ArrowDown) && count > 0 {
                    self.picker.selected = (self.picker.selected + 1) % count;
                }
                if i.key_pressed(Key::ArrowUp) && count > 0 {
                    self.picker.selected = (self.picker.selected + count - 1) % count;
                }
                if i.key_pressed(Key::Enter) && count > 0 {
                    outcome = Some(Outcome::Choose(self.picker.selected));
                }
                let digits = [
                    Key::Num1,
                    Key::Num2,
                    Key::Num3,
                    Key::Num4,
                    Key::Num5,
                    Key::Num6,
                    Key::Num7,
                    Key::Num8,
                    Key::Num9,
                ];
                for (n, key) in digits.iter().enumerate() {
                    if i.key_pressed(*key) && n < count {
                        outcome = Some(Outcome::Choose(n));
                    }
                }
                match i.viewport().focused {
                    Some(true) => self.picker.seen_focus = true,
                    Some(false) if self.picker.seen_focus => outcome = Some(Outcome::Cancel),
                    _ => {}
                }
            });

            egui::Frame::central_panel(ui.style()).show(ui, |ui| {
                ui.add(egui::Label::new(RichText::new(url).small().weak()).truncate());
                ui.add_space(4.0);
                for (n, choice) in core.picker_choices.iter().enumerate() {
                    let selected = n == self.picker.selected;
                    let icon = self.icons.get(&choice.bundle_id);
                    let response = choice_row(ui, choice, icon, n, selected);
                    if response.hovered() {
                        self.picker.selected = n;
                    }
                    if response.clicked() {
                        outcome = Some(Outcome::Choose(n));
                    }
                }
                if count == 0 {
                    ui.label("ブラウザが見つかりません");
                }
            });
        }

        let Some(outcome) = outcome else { return };
        let (pending, target) = {
            let mut c = self.core.borrow_mut();
            let target = match outcome {
                Outcome::Choose(n) => c.picker_choices.get(n).map(|ch| ch.target.clone()),
                Outcome::Cancel => None,
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
            .get_or_insert_with(|| SettingsState::new(self.core.borrow().store.config()));
        let mut after = SettingsAfter::default();
        {
            let core = self.core.borrow();
            egui::Frame::central_panel(ui.style()).show(ui, |ui| {
                settings.ui(ui, &core, &mut after);
            });
        }
        if let Some(config) = after.save {
            let result = self.core.borrow_mut().store.save(config);
            settings.message = Some(match result {
                Ok(()) => ("保存しました".into(), false),
                Err(e) => (format!("保存に失敗しました: {e}"), true),
            });
        }
        if after.reload {
            let mut c = self.core.borrow_mut();
            c.store.reload();
            c.rescan_browsers();
            *settings = SettingsState::new(c.store.config());
        }
        if after.set_default {
            macos::set_as_default_browser();
        }
        if let Some(enabled) = after.login_item {
            if let Err(e) = macos::set_login_item(enabled) {
                settings.message = Some((format!("ログイン項目の変更に失敗しました: {e}"), true));
            }
            settings.login_item = macos::login_item_status();
        }
    }
}

impl eframe::App for App {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // ウィンドウを閉じてもアプリは常駐させ続ける
        if ctx.input(|i| i.viewport().close_requested()) {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            present(&self.core, Mode::Hidden);
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.sync_icons(ui.ctx());
        let mode = self.core.borrow().mode;
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

fn choice_row(
    ui: &mut egui::Ui,
    choice: &Choice,
    icon: Option<&TextureHandle>,
    index: usize,
    selected: bool,
) -> egui::Response {
    let width = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(width, PICKER_ROW_HEIGHT as f32),
        egui::Sense::click(),
    );
    if selected {
        ui.painter().rect_filled(
            rect,
            6.0,
            ui.visuals().selection.bg_fill.gamma_multiply(0.6),
        );
    }
    let icon_size = 24.0;
    let icon_rect = egui::Rect::from_min_size(
        rect.left_center() + egui::vec2(8.0, -icon_size / 2.0),
        egui::vec2(icon_size, icon_size),
    );
    if let Some(icon) = icon {
        egui::Image::new(icon).paint_at(ui, icon_rect);
    }
    let text_color = ui.visuals().strong_text_color();
    ui.painter().text(
        icon_rect.right_center() + egui::vec2(10.0, 0.0),
        egui::Align2::LEFT_CENTER,
        &choice.label,
        egui::FontId::proportional(14.0),
        text_color,
    );
    if index < 9 {
        ui.painter().text(
            rect.right_center() - egui::vec2(10.0, 0.0),
            egui::Align2::RIGHT_CENTER,
            (index + 1).to_string(),
            egui::FontId::monospace(12.0),
            ui.visuals().weak_text_color(),
        );
    }
    response
}

#[derive(Default)]
struct SettingsAfter {
    save: Option<Config>,
    reload: bool,
    set_default: bool,
    login_item: Option<bool>,
}

struct SettingsState {
    default: Action,
    alternative: Option<Action>,
    picker_items: Vec<Target>,
    rules: Vec<RuleDraft>,
    running_apps: Vec<macos::RunningApp>,
    is_default_browser: bool,
    login_item: LoginItemStatus,
    message: Option<(String, bool)>,
    test_url: String,
    test_source: String,
    test_fn: bool,
}

/// 複数行テキストを編集中にそのまま保持するための、Rule の編集用表現
#[derive(Clone, PartialEq)]
struct RuleDraft {
    name: String,
    hosts: String,
    urls: String,
    source_apps: String,
    action: Action,
}

impl RuleDraft {
    fn from_rule(rule: &Rule) -> Self {
        Self {
            name: rule.name.clone(),
            hosts: rule.hosts.join("\n"),
            urls: rule.urls.join("\n"),
            source_apps: rule.source_apps.join("\n"),
            action: rule.action.clone(),
        }
    }

    fn to_rule(&self) -> Rule {
        let lines = |s: &str| {
            s.lines()
                .map(str::trim)
                .filter(|l| !l.is_empty())
                .map(str::to_owned)
                .collect()
        };
        Rule {
            name: self.name.trim().to_owned(),
            hosts: lines(&self.hosts),
            urls: lines(&self.urls),
            source_apps: lines(&self.source_apps),
            action: self.action.clone(),
        }
    }
}

impl SettingsState {
    fn new(config: &Config) -> Self {
        Self {
            default: config.default.clone(),
            alternative: config.alternative.clone(),
            picker_items: config.picker_items.clone(),
            rules: config.rules.iter().map(RuleDraft::from_rule).collect(),
            running_apps: macos::running_apps(),
            is_default_browser: macos::is_default_browser(),
            login_item: macos::login_item_status(),
            message: None,
            test_url: String::new(),
            test_source: String::new(),
            test_fn: false,
        }
    }

    fn to_config(&self) -> Config {
        Config {
            default: self.default.clone(),
            alternative: self.alternative.clone(),
            picker_items: self.picker_items.clone(),
            rules: self.rules.iter().map(RuleDraft::to_rule).collect(),
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, core: &Core, after: &mut SettingsAfter) {
        let draft = self.to_config();
        let dirty = &draft != core.store.config();

        egui::Panel::bottom("settings-footer").show(ui, |ui| {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if ui.add_enabled(dirty, egui::Button::new("保存")).clicked() {
                    after.save = Some(draft.clone());
                }
                if ui
                    .add_enabled(dirty, egui::Button::new("変更を破棄"))
                    .clicked()
                {
                    after.reload = true;
                }
                if dirty {
                    ui.label(
                        RichText::new("未保存の変更があります")
                            .color(Color32::from_rgb(220, 160, 40)),
                    );
                } else if let Some((msg, is_err)) = &self.message {
                    let color = if *is_err {
                        Color32::RED
                    } else {
                        ui.visuals().weak_text_color()
                    };
                    ui.label(RichText::new(msg).color(color));
                }
            });
        });

        egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
            ui.heading("一般");
            egui::Grid::new("general").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
                ui.label("デフォルトブラウザ");
                ui.horizontal(|ui| {
                    if self.is_default_browser {
                        ui.label("設定済み");
                    } else {
                        ui.label(RichText::new("未設定").color(Color32::from_rgb(220, 160, 40)));
                        if ui.button("デフォルトブラウザに設定").clicked() {
                            after.set_default = true;
                        }
                    }
                });
                ui.end_row();

                ui.label("ログイン時に起動");
                ui.horizontal(|ui| {
                    let mut enabled = self.login_item != LoginItemStatus::Disabled;
                    if ui.checkbox(&mut enabled, "").changed() {
                        after.login_item = Some(enabled);
                    }
                    if self.login_item == LoginItemStatus::RequiresApproval {
                        ui.label(RichText::new("システム設定での許可が必要です").color(Color32::from_rgb(220, 160, 40)));
                        if ui.small_button("ログイン項目を開く").clicked() {
                            macos::open_login_items_settings();
                        }
                    }
                });
                ui.end_row();

                ui.label("設定ファイル");
                ui.horizontal(|ui| {
                    ui.monospace(core.store.path().to_string_lossy());
                    if ui.small_button("開く").clicked() {
                        macos::open_file(core.store.path());
                    }
                    if ui.small_button("Finderで表示").clicked() {
                        macos::reveal_in_finder(core.store.path());
                    }
                    if ui.small_button("再読み込み").clicked() {
                        after.reload = true;
                    }
                });
                ui.end_row();

                ui.label("マッチしないとき");
                action_combo(ui, "default-action", &mut self.default, core);
                ui.end_row();

                ui.label("Fn を押しながら").on_hover_text("ルールより優先して使います");
                ui.horizontal(|ui| {
                    let mut enabled = self.alternative.is_some();
                    if ui.checkbox(&mut enabled, "").changed() {
                        self.alternative = enabled.then_some(Action::Picker);
                    }
                    match &mut self.alternative {
                        Some(action) => action_combo(ui, "alternative-action", action, core),
                        None => {
                            ui.label(RichText::new("使わない").weak());
                        }
                    }
                });
                ui.end_row();
            });
            if let Some(err) = core.store.error() {
                ui.colored_label(Color32::RED, format!("設定ファイルの読み込みに失敗しました (前回の設定を使用中):\n{err}"));
            }

            ui.add_space(12.0);
            ui.heading("選択UI");
            picker_items_editor(ui, &mut self.picker_items, core);

            ui.add_space(12.0);
            ui.heading("ルール");
            ui.label(RichText::new("上から順に評価し、最初にマッチしたルールで開きます。空欄の条件は「何でもよい」として扱いますが、条件が1つもないルールは何にもマッチしません。").weak());
            ui.add_space(4.0);

            let mut move_op: Option<(usize, isize)> = None;
            let mut remove: Option<usize> = None;
            let count = self.rules.len();
            for (i, rule) in self.rules.iter_mut().enumerate() {
                egui::Frame::group(ui.style()).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        ui.label(format!("#{}", i + 1));
                        ui.add(egui::TextEdit::singleline(&mut rule.name).hint_text("ルール名").desired_width(240.0));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.small_button("削除").clicked() {
                                remove = Some(i);
                            }
                            if ui.add_enabled(i + 1 < count, egui::Button::new("↓").small()).clicked() {
                                move_op = Some((i, 1));
                            }
                            if ui.add_enabled(i > 0, egui::Button::new("↑").small()).clicked() {
                                move_op = Some((i, -1));
                            }
                        });
                    });
                    rule_editor(ui, i, rule, core, &self.running_apps);
                });
                ui.add_space(6.0);
            }
            if let Some(i) = remove {
                self.rules.remove(i);
            }
            if let Some((i, d)) = move_op {
                self.rules.swap(i, i.saturating_add_signed(d));
            }
            if ui.button("＋ ルールを追加").clicked() {
                self.rules.push(RuleDraft::from_rule(&Rule::default()));
            }

            ui.add_space(12.0);
            ui.heading("テスト");
            ui.label(RichText::new("編集中 (未保存) のルールで、どのブラウザが選ばれるかを確認できます。").weak());
            egui::Grid::new("test").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
                ui.label("URL");
                ui.add(egui::TextEdit::singleline(&mut self.test_url).hint_text("https://example.com/").desired_width(f32::INFINITY));
                ui.end_row();
                ui.label("開き元アプリ");
                ui.add(egui::TextEdit::singleline(&mut self.test_source).hint_text("com.tinyspeck.slackmacgap").desired_width(f32::INFINITY));
                ui.end_row();
                ui.label("Fn");
                ui.checkbox(&mut self.test_fn, "押している");
                ui.end_row();
                ui.label("結果");
                if self.test_url.trim().is_empty() {
                    ui.label("—");
                } else {
                    let req = Request {
                        url: self.test_url.trim().to_owned(),
                        source_app: Some(self.test_source.trim().to_owned()).filter(|s| !s.is_empty()),
                        fn_pressed: self.test_fn,
                    };
                    let hit = draft.rules.iter().position(|r| router::matches(r, &req));
                    let action = router::route(&draft, &req);
                    let by = match hit {
                        _ if req.fn_pressed && draft.alternative.is_some() => "Fn".into(),
                        Some(i) => format!("ルール #{}", i + 1),
                        None => "マッチなし".into(),
                    };
                    ui.label(format!("{} ({by})", browsers::describe(&core.browsers, action)));
                }
                ui.end_row();
            });
        });
    }
}

fn rule_editor(
    ui: &mut egui::Ui,
    i: usize,
    rule: &mut RuleDraft,
    core: &Core,
    running_apps: &[macos::RunningApp],
) {
    egui::Grid::new(("rule", i))
        .num_columns(2)
        .spacing([12.0, 6.0])
        .show(ui, |ui| {
            ui.label("ホスト")
                .on_hover_text("1行に1つ。* はワイルドカード。例: *.example.com");
            ui.add(
                egui::TextEdit::multiline(&mut rule.hosts)
                    .desired_rows(1)
                    .hint_text("*.example.com")
                    .desired_width(f32::INFINITY),
            );
            ui.end_row();

            ui.label("URL")
                .on_hover_text("URL全体に対するパターン。1行に1つ");
            ui.add(
                egui::TextEdit::multiline(&mut rule.urls)
                    .desired_rows(1)
                    .hint_text("https://github.com/my-org/*")
                    .desired_width(f32::INFINITY),
            );
            ui.end_row();

            ui.label("開き元アプリ")
                .on_hover_text("バンドルID。1行に1つ");
            ui.vertical(|ui| {
                ui.add(
                    egui::TextEdit::multiline(&mut rule.source_apps)
                        .desired_rows(1)
                        .hint_text("com.tinyspeck.slackmacgap")
                        .desired_width(f32::INFINITY),
                );
                egui::ComboBox::from_id_salt(("add-app", i))
                    .selected_text("起動中のアプリから追加…")
                    .show_ui(ui, |ui| {
                        for app in running_apps {
                            if ui
                                .selectable_label(
                                    false,
                                    format!("{} ({})", app.name, app.bundle_id),
                                )
                                .clicked()
                            {
                                if !rule.source_apps.is_empty() && !rule.source_apps.ends_with('\n')
                                {
                                    rule.source_apps.push('\n');
                                }
                                rule.source_apps.push_str(&app.bundle_id);
                            }
                        }
                    });
            });
            ui.end_row();

            ui.label("開き方");
            action_combo(ui, ("action", i), &mut rule.action, core);
            ui.end_row();
        });
}

fn picker_items_editor(ui: &mut egui::Ui, items: &mut Vec<Target>, core: &Core) {
    let mut auto = items.is_empty();
    if ui
        .checkbox(&mut auto, "検出したブラウザとプロファイルをすべて表示する")
        .changed()
    {
        *items = if auto {
            Vec::new()
        } else {
            core.all_choices.iter().map(|c| c.target.clone()).collect()
        };
    }
    if auto {
        return;
    }
    ui.label(RichText::new("上から順に並び、先頭9件は数字キーで選べます。").weak());

    let mut move_op: Option<(usize, isize)> = None;
    let mut remove: Option<usize> = None;
    let count = items.len();
    for (i, target) in items.iter().enumerate() {
        ui.horizontal(|ui| {
            ui.label(format!("{}.", i + 1));
            ui.label(browsers::describe_target(&core.browsers, target));
            if !core
                .browsers
                .iter()
                .any(|b| b.bundle_id == target.bundle_id)
            {
                ui.label(RichText::new("(未インストール)").weak());
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button("削除").clicked() {
                    remove = Some(i);
                }
                if ui
                    .add_enabled(i + 1 < count, egui::Button::new("↓").small())
                    .clicked()
                {
                    move_op = Some((i, 1));
                }
                if ui
                    .add_enabled(i > 0, egui::Button::new("↑").small())
                    .clicked()
                {
                    move_op = Some((i, -1));
                }
            });
        });
    }
    if let Some(i) = remove {
        items.remove(i);
    }
    if let Some((i, d)) = move_op {
        items.swap(i, i.saturating_add_signed(d));
    }

    let addable: Vec<&Choice> = core
        .all_choices
        .iter()
        .filter(|c| !items.contains(&c.target))
        .collect();
    if !addable.is_empty() {
        egui::ComboBox::from_id_salt("add-picker-item")
            .selected_text("追加…")
            .width(320.0)
            .show_ui(ui, |ui| {
                for choice in addable {
                    if ui.selectable_label(false, &choice.label).clicked() {
                        items.push(choice.target.clone());
                    }
                }
            });
    }
}

fn action_combo(ui: &mut egui::Ui, id: impl egui::AsIdSalt, action: &mut Action, core: &Core) {
    egui::ComboBox::from_id_salt(id)
        .selected_text(browsers::describe(&core.browsers, action))
        .width(320.0)
        .show_ui(ui, |ui| {
            ui.selectable_value(action, Action::Picker, "選択UIを表示");
            for choice in &core.all_choices {
                ui.selectable_value(
                    action,
                    Action::Browser(choice.target.clone()),
                    &choice.label,
                );
            }
        });
}

fn install_japanese_font(ctx: &egui::Context) {
    // egui同梱フォントには日本語グリフがない
    let Ok(bytes) = std::fs::read("/System/Library/Fonts/ヒラギノ角ゴシック W3.ttc")
    else {
        return;
    };
    let mut fonts = egui::FontDefinitions::default();
    fonts
        .font_data
        .insert("hiragino".into(), egui::FontData::from_owned(bytes).into());
    for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
        fonts
            .families
            .entry(family)
            .or_default()
            .push("hiragino".into());
    }
    ctx.set_fonts(fonts);
}
