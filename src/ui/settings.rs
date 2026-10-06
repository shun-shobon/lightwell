use std::time::{Duration, Instant};

use eframe::egui::{self, Color32, CornerRadius, Frame, Id, Margin, RichText, Sense, Ui, vec2};
use egui_flex::{Flex, FlexAlign, FlexInstance, item};

use super::Textures;
use super::theme::{Palette, Typography};
use super::widgets::{self, PopupOption, footnote, section_header};
use crate::app::Core;
use crate::browsers;
use crate::config::{Action, Config, Rule, Target};
use crate::macos::{self, LoginItemStatus, RunningApp};

pub const SIZE: (f64, f64) = (760.0, 540.0);
pub const MIN_SIZE: (f64, f64) = (640.0, 420.0);
const SIDEBAR_WIDTH: f32 = 200.0;
/// 信号機ボタンが乗るタイトルバー部分の高さ
const TITLEBAR_HEIGHT: f32 = 52.0;
const AUTOSAVE_DELAY: Duration = Duration::from_millis(400);
const WARNING: Color32 = Color32::from_rgb(230, 140, 20);

#[derive(Clone, Copy, PartialEq, Eq)]
enum Page {
    General,
    Picker,
    Rules,
    Rule(usize),
}

/// 描画後に App 側で行う副作用。描画中は Core を借用しているので、ここに積んでおく
#[derive(Default)]
pub struct Effects {
    pub save: Option<Config>,
    pub set_default_browser: bool,
    pub login_item: Option<bool>,
    pub choose_source_app: Option<usize>,
}

pub struct Settings {
    page: Page,
    pub draft: Config,
    pending: Option<(Config, Instant)>,
    running_apps: Vec<RunningApp>,
    is_default_browser: bool,
    pub login_item: LoginItemStatus,
    pub error: Option<String>,
    focus: Option<Id>,
}

impl Settings {
    pub fn new(config: &Config) -> Self {
        #[cfg(debug_assertions)]
        let page = match std::env::var("LIGHTWELL_PREVIEW").as_deref() {
            Ok("settings:picker") => Page::Picker,
            Ok("settings:rules") => Page::Rules,
            Ok("settings:rule") => Page::Rule(0),
            _ => Page::General,
        };
        #[cfg(not(debug_assertions))]
        let page = Page::General;
        Self {
            page,
            draft: config.clone(),
            pending: None,
            running_apps: macos::running_apps(),
            is_default_browser: macos::is_default_browser(),
            login_item: macos::login_item_status(),
            error: None,
            focus: None,
        }
    }

    pub fn show(&mut self, ui: &mut Ui, core: &Core, textures: &mut Textures) -> Effects {
        let mut fx = Effects::default();
        titlebar_drag_area(ui);
        // サイドバーは塗らずに背面のすりガラスを透かす
        egui::Panel::left("sidebar")
            .exact_size(SIDEBAR_WIDTH)
            .resizable(false)
            .frame(Frame::new().inner_margin(Margin {
                left: 10,
                right: 10,
                top: TITLEBAR_HEIGHT as i8,
                bottom: 0,
            }))
            .show(ui, |ui| self.sidebar(ui, textures));
        egui::CentralPanel::default()
            .frame(Frame::new().fill(Palette::of(ui).window))
            .show(ui, |ui| self.content(ui, core, textures, &mut fx));
        self.autosave(ui.ctx(), core, &mut fx);
        fx
    }

    fn autosave(&mut self, ctx: &egui::Context, core: &Core, fx: &mut Effects) {
        let normalized = normalize(&self.draft);
        if normalized == *core.store.config() {
            self.pending = None;
            return;
        }
        let now = Instant::now();
        let since = match &self.pending {
            Some((pending, since)) if *pending == normalized => *since,
            _ => {
                self.pending = Some((normalized.clone(), now));
                now
            }
        };
        if now.duration_since(since) >= AUTOSAVE_DELAY {
            fx.save = Some(normalized);
            self.pending = None;
        } else {
            ctx.request_repaint_after(AUTOSAVE_DELAY - now.duration_since(since));
        }
    }

    fn sidebar(&mut self, ui: &mut Ui, textures: &mut Textures) {
        let p = Palette::of(ui);
        ui.spacing_mut().item_spacing.y = 2.0;
        let pages = [
            (
                Page::General,
                "一般",
                "gearshape.fill",
                Color32::from_rgb(142, 142, 147),
            ),
            (
                Page::Picker,
                "選択UI",
                "square.grid.2x2.fill",
                Color32::from_rgb(10, 132, 255),
            ),
            (
                Page::Rules,
                "ルール",
                "arrow.triangle.branch",
                Color32::from_rgb(255, 149, 0),
            ),
        ];
        for (page, label, symbol, color) in pages {
            let selected = match self.page {
                Page::Rule(_) => page == Page::Rules,
                current => current == page,
            };
            let badge = textures.symbol(ui.ctx(), symbol);
            let response = Frame::new()
                .fill(if selected {
                    p.accent
                } else {
                    Color32::TRANSPARENT
                })
                .corner_radius(CornerRadius::same(6))
                .inner_margin(Margin::symmetric(6, 5))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    Flex::horizontal()
                        .align_items(FlexAlign::Center)
                        .gap(vec2(8.0, 0.0))
                        .show(ui, |f| {
                            f.add_ui(item(), |ui| {
                                widgets::symbol_badge(ui, badge.as_ref(), color, 20.0)
                            });
                            let text = RichText::new(label);
                            f.add(
                                item(),
                                egui::Label::new(if selected {
                                    text.color(Color32::WHITE)
                                } else {
                                    text
                                }),
                            );
                        });
                })
                .response
                .interact(Sense::click());
            if response.clicked() {
                self.page = page;
            }
        }
    }

    fn content(&mut self, ui: &mut Ui, core: &Core, textures: &mut Textures, fx: &mut Effects) {
        egui::Panel::top("title")
            .exact_size(TITLEBAR_HEIGHT)
            .show_separator_line(false)
            .frame(Frame::new().inner_margin(Margin::symmetric(20, 0)))
            .show(ui, |ui| self.title(ui, textures));
        egui::ScrollArea::vertical()
            .auto_shrink(false)
            .show(ui, |ui| {
                Frame::new()
                    .inner_margin(Margin {
                        left: 20,
                        right: 20,
                        top: 0,
                        bottom: 24,
                    })
                    .show(ui, |ui| match self.page {
                        Page::General => self.general(ui, core, textures, fx),
                        Page::Picker => self.picker(ui, core, textures),
                        Page::Rules => self.rules(ui, core, textures),
                        Page::Rule(i) => self.rule(ui, i, core, textures, fx),
                    });
            });
    }

    fn title(&mut self, ui: &mut Ui, textures: &mut Textures) {
        let title = match self.page {
            Page::General => "一般",
            Page::Picker => "選択UI",
            Page::Rules => "ルール",
            Page::Rule(i) => self.draft.rules.get(i).map_or("", display_name),
        }
        .to_owned();
        let secondary = Palette::of(ui).secondary;
        let back_icon = textures.symbol(ui.ctx(), "chevron.left");
        Flex::horizontal()
            .h_full()
            .align_items(FlexAlign::Center)
            .gap(vec2(6.0, 0.0))
            .show(ui, |f| {
                if let Page::Rule(_) = self.page {
                    let back = f.add_ui(item(), |ui| {
                        widgets::symbol_button(ui, back_icon.as_ref(), secondary)
                    });
                    if back.inner.clicked() {
                        self.page = Page::Rules;
                    }
                }
                f.add(
                    item(),
                    egui::Label::new(RichText::new(title).text_style(Typography::Title.style())),
                );
            });
    }

    fn general(&mut self, ui: &mut Ui, core: &Core, textures: &mut Textures, fx: &mut Effects) {
        let p = Palette::of(ui);
        widgets::group(ui, |g| {
            let is_default = self.is_default_browser;
            let detail = if is_default {
                "リンクは Lightwell が受け取って振り分けます"
            } else {
                "リンクを振り分けるには、Lightwell をデフォルトブラウザにします"
            };
            g.described("デフォルトブラウザ", detail, |ui| {
                if is_default {
                    ui.label(RichText::new("設定済み").color(p.secondary));
                } else if ui.button("デフォルトにする").clicked() {
                    fx.set_default_browser = true;
                }
            });

            let requires_approval = self.login_item == LoginItemStatus::RequiresApproval;
            let detail = if requires_approval {
                "システム設定のログイン項目で許可してください"
            } else {
                "常駐させておくと、リンクを開いたときに待たされません"
            };
            g.described("ログイン時に起動", detail, |ui| {
                ui.horizontal(|ui| {
                    if requires_approval && ui.button("開く").clicked() {
                        macos::open_login_items_settings();
                    }
                    let mut on = self.login_item != LoginItemStatus::Disabled;
                    if widgets::toggle(ui, &mut on).changed() {
                        fx.login_item = Some(on);
                    }
                });
            });
        });

        section_header(ui, "リンクの開き方");
        let chevron = textures.symbol(ui.ctx(), "chevron.up.chevron.down");
        let actions = action_options(core, textures, ui.ctx());
        widgets::group(ui, |g| {
            g.described(
                "通常",
                "どのルールにも一致しないとき",
                |ui| {
                    widgets::popup(
                        ui,
                        &mut self.draft.default,
                        &actions,
                        "不明なブラウザ",
                        chevron.as_ref(),
                    );
                },
            );
            g.described(
                "Fn キーを押しながら",
                "ルールより優先されます",
                |ui| {
                    let options: Vec<_> = std::iter::once(PopupOption {
                        value: None,
                        label: "使わない".to_owned(),
                        icon: None,
                    })
                    .chain(actions.iter().map(|o| PopupOption {
                        value: Some(o.value.clone()),
                        label: o.label.clone(),
                        icon: o.icon.clone(),
                    }))
                    .collect();
                    widgets::popup(
                        ui,
                        &mut self.draft.alternative,
                        &options,
                        "不明なブラウザ",
                        chevron.as_ref(),
                    );
                },
            );
        });

        section_header(ui, "設定ファイル");
        widgets::group(ui, |g| {
            g.row(|f| {
                let path = RichText::new(core.store.path().to_string_lossy()).color(p.secondary);
                widgets::fill_text(f, path);
                if f.add(item(), egui::Button::new("Finder で表示")).clicked() {
                    macos::reveal_in_finder(core.store.path());
                }
            });
        });
        footnote(
            ui,
            "変更は自動で保存されます。ファイルを直接編集した内容も、次にリンクを開いたときに反映されます。",
        );
        if let Some(err) = core.store.error() {
            error_note(
                ui,
                &format!("設定ファイルを読み込めませんでした。直前の設定を使っています。\n{err}"),
            );
        }
        if let Some(err) = &self.error {
            error_note(ui, err);
        }
    }

    fn picker(&mut self, ui: &mut Ui, core: &Core, textures: &mut Textures) {
        let p = Palette::of(ui);
        let (mut shown, hidden) =
            browsers::picker_partition(&core.all_choices, self.draft.picker_items.as_deref());
        let before = shown.clone();
        let installed = |t: &Target| core.browsers.iter().any(|b| b.bundle_id == t.bundle_id);

        widgets::group(ui, |g| {
            let mut uncheck = None;
            widgets::reorderable(g, Id::new("picker-items"), &mut shown, |f, i, target| {
                let mut on = true;
                if f.add_ui(item(), |ui| widgets::checkbox(ui, &mut on))
                    .inner
                    .changed()
                {
                    uncheck = Some(i);
                }
                browser_label(f, core, textures, target, p.text);
                if !installed(target) {
                    f.add(
                        item(),
                        egui::Label::new(
                            RichText::new("未インストール").small().color(p.secondary),
                        ),
                    );
                }
                if i < 9 {
                    f.add(
                        item(),
                        egui::Label::new(RichText::new((i + 1).to_string()).color(p.tertiary)),
                    );
                }
            });
            if let Some(i) = uncheck {
                shown.remove(i);
            }
            for target in &hidden {
                g.row(|f| {
                    f.add_ui(item(), widgets::drag_handle_spacer);
                    let mut on = false;
                    if f.add_ui(item(), |ui| widgets::checkbox(ui, &mut on))
                        .inner
                        .changed()
                    {
                        shown.push(target.clone());
                    }
                    browser_label(f, core, textures, target, p.secondary);
                });
            }
        });
        if shown != before {
            self.draft.picker_items = Some(shown);
        }
        footnote(
            ui,
            "チェックしたものが、この順番で選択UIに並びます。ドラッグで並べ替えられます。先頭9件は数字キーで選べます。",
        );
    }

    fn rules(&mut self, ui: &mut Ui, core: &Core, textures: &mut Textures) {
        let p = Palette::of(ui);
        let chevron = textures.symbol(ui.ctx(), "chevron.right");
        let mut open = None;
        widgets::group(ui, |g| {
            if self.draft.rules.is_empty() {
                g.row(|f| {
                    widgets::fill_text(
                        f,
                        RichText::new("ルールはまだありません").color(p.secondary),
                    );
                });
                return;
            }
            widgets::reorderable(g, Id::new("rules"), &mut self.draft.rules, |f, i, rule| {
                let summary = if rule.has_conditions() {
                    RichText::new(summarize(rule, textures))
                        .small()
                        .color(p.secondary)
                } else {
                    RichText::new("条件がないため使われません")
                        .small()
                        .color(WARNING)
                };
                let name = RichText::new(display_name(rule));
                let name = if rule.name.trim().is_empty() {
                    name.color(p.secondary)
                } else {
                    name
                };
                let target_icon = match &rule.action {
                    Action::Browser(t) => textures.browser_icon(f.ui().ctx(), &t.bundle_id),
                    Action::Picker => None,
                };
                let row = f.add_flex(
                    item().grow(1.0).sense(Sense::click()),
                    Flex::horizontal()
                        .align_items(FlexAlign::Center)
                        .gap(vec2(8.0, 0.0)),
                    |f| {
                        widgets::text_column(f, vec![name.into(), summary.into()]);
                        if target_icon.is_some() {
                            f.add_widget(item(), widgets::icon(target_icon.as_ref(), 18.0));
                        }
                        f.add(
                            item(),
                            egui::Label::new(
                                RichText::new(browsers::describe(&core.browsers, &rule.action))
                                    .color(p.secondary),
                            ),
                        );
                        f.add_widget(item(), widgets::symbol(chevron.as_ref(), 10.0, p.tertiary));
                    },
                );
                if row.response.clicked() {
                    open = Some(i);
                }
            });
        });
        ui.add_space(10.0);
        if ui.button("＋ ルールを追加").clicked() {
            self.draft.rules.push(Rule {
                name: "新しいルール".to_owned(),
                ..Default::default()
            });
            open = Some(self.draft.rules.len() - 1);
        }
        footnote(
            ui,
            "上から順に評価し、最初に一致したルールのブラウザで開きます。ドラッグで並べ替えられます。",
        );
        if let Some(i) = open {
            self.page = Page::Rule(i);
        }
    }

    fn rule(
        &mut self,
        ui: &mut Ui,
        i: usize,
        core: &Core,
        textures: &mut Textures,
        fx: &mut Effects,
    ) {
        let p = Palette::of(ui);
        let minus = textures.symbol(ui.ctx(), "minus.circle.fill");
        let chevron = textures.symbol(ui.ctx(), "chevron.up.chevron.down");
        let actions = action_options(core, textures, ui.ctx());
        let Some(rule) = self.draft.rules.get_mut(i) else {
            self.page = Page::Rules;
            return;
        };

        widgets::group(ui, |g| {
            g.labeled("名前", |ui| {
                ui.add(egui::TextEdit::singleline(&mut rule.name).desired_width(260.0));
            });
        });

        let patterns = [
            (
                "ホスト",
                "*.example.com",
                "* は任意の文字列に一致します。*.example.com は example.com 自体には一致しません",
                &mut rule.hosts,
                "＋ ホストを追加",
            ),
            (
                "URL",
                "https://github.com/my-org/*",
                "URL全体に対するパターンです",
                &mut rule.urls,
                "＋ URLを追加",
            ),
        ];
        for (title, hint, caption, list, add_label) in patterns {
            section_header(ui, title);
            widgets::group(ui, |g| {
                let mut remove = None;
                for (j, value) in list.iter_mut().enumerate() {
                    let id = Id::new((title, i, j));
                    g.row(|f| {
                        let edit = egui::TextEdit::singleline(value).id(id).hint_text(hint);
                        let response = f.add(widgets::fill(), edit);
                        if self.focus == Some(id) {
                            response.request_focus();
                            self.focus = None;
                        }
                        if f.add_ui(item(), |ui| {
                            widgets::symbol_button(ui, minus.as_ref(), p.tertiary)
                        })
                        .inner
                        .clicked()
                        {
                            remove = Some(j);
                        }
                    });
                }
                if let Some(j) = remove {
                    list.remove(j);
                }
                g.row(|f| {
                    if f.add_ui(item(), |ui| ui.add(widgets::link_button(add_label)))
                        .inner
                        .clicked()
                    {
                        list.push(String::new());
                        self.focus = Some(Id::new((title, i, list.len() - 1)));
                    }
                });
            });
            footnote(ui, caption);
        }

        section_header(ui, "開き元アプリ");
        widgets::group(ui, |g| {
            let mut remove = None;
            for (j, bundle_id) in rule.source_apps.iter().enumerate() {
                g.row(|f| {
                    let icon = textures.app_icon(f.ui().ctx(), bundle_id);
                    f.add_widget(item(), widgets::icon(icon.as_ref(), 22.0));
                    f.add(item(), egui::Label::new(textures.app_name(bundle_id)));
                    widgets::fill_text(f, RichText::new(bundle_id).small().color(p.tertiary));
                    if f.add_ui(item(), |ui| {
                        widgets::symbol_button(ui, minus.as_ref(), p.tertiary)
                    })
                    .inner
                    .clicked()
                    {
                        remove = Some(j);
                    }
                });
            }
            if let Some(j) = remove {
                rule.source_apps.remove(j);
            }
            g.row(|f| {
                f.add_ui(item(), |ui| {
                    add_source_app_menu(ui, rule, &self.running_apps, textures, i, fx);
                });
            });
        });
        footnote(ui, "どのアプリでリンクをクリックしたかで振り分けます");

        section_header(ui, "開くブラウザ");
        widgets::group(ui, |g| {
            g.labeled("ブラウザ", |ui| {
                widgets::popup(
                    ui,
                    &mut rule.action,
                    &actions,
                    "不明なブラウザ",
                    chevron.as_ref(),
                );
            });
        });

        if !rule.has_conditions() {
            ui.add_space(8.0);
            ui.label(
                RichText::new(
                    "ホスト・URL・開き元アプリのいずれかを設定するまで、このルールは使われません",
                )
                .small()
                .color(WARNING),
            );
        }

        ui.add_space(20.0);
        if ui
            .button(RichText::new("ルールを削除").color(p.destructive))
            .clicked()
        {
            self.draft.rules.remove(i);
            self.page = Page::Rules;
        }
    }
}

fn add_source_app_menu(
    ui: &mut Ui,
    rule: &mut Rule,
    running_apps: &[RunningApp],
    textures: &mut Textures,
    rule_index: usize,
    fx: &mut Effects,
) {
    let p = Palette::of(ui);
    let button = egui::Button::new(RichText::new("＋ アプリを追加").color(p.accent)).frame(false);
    egui::containers::menu::MenuButton::from_button(button).ui(ui, |ui| {
        ui.set_min_width(240.0);
        ui.label(RichText::new("起動中のアプリ").small().color(p.secondary));
        for app in running_apps {
            if rule.source_apps.contains(&app.bundle_id) {
                continue;
            }
            let mut atoms = egui::Atoms::default();
            if let Some(icon) = textures.app_icon(ui.ctx(), &app.bundle_id) {
                atoms
                    .push_right(egui::Image::new(&icon).fit_to_exact_size(egui::Vec2::splat(16.0)));
            }
            atoms.push_right(app.name.as_str());
            if ui.add(egui::Button::new(atoms)).clicked() {
                rule.source_apps.push(app.bundle_id.clone());
                ui.close();
            }
        }
        ui.separator();
        if ui.button("その他のアプリ…").clicked() {
            fx.choose_source_app = Some(rule_index);
            ui.close();
        }
    });
}

/// タイトルバー部分は egui が覆っているので、何もない所のドラッグでウィンドウを動かせるようにする。
/// 後から置くウィジェットが優先されるよう、最初に登録する
fn titlebar_drag_area(ui: &mut Ui) {
    let rect = ui
        .max_rect()
        .with_max_y(ui.max_rect().top() + TITLEBAR_HEIGHT);
    if ui
        .interact(rect, Id::new("titlebar-drag"), Sense::drag())
        .drag_started()
    {
        ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
    }
}

fn browser_label(
    f: &mut FlexInstance,
    core: &Core,
    textures: &mut Textures,
    target: &Target,
    color: Color32,
) {
    let icon = textures.browser_icon(f.ui().ctx(), &target.bundle_id);
    f.add_widget(item(), widgets::icon(icon.as_ref(), 22.0));
    widgets::fill_text(
        f,
        RichText::new(browsers::describe_target(&core.browsers, target)).color(color),
    );
}

fn error_note(ui: &mut Ui, text: &str) {
    ui.add_space(8.0);
    ui.label(
        RichText::new(text)
            .small()
            .color(Palette::of(ui).destructive),
    );
}

fn display_name(rule: &Rule) -> &str {
    match rule.name.trim() {
        "" => "名称未設定",
        name => name,
    }
}

fn summarize(rule: &Rule, textures: &mut Textures) -> String {
    let list = |items: &[String]| {
        let items: Vec<&str> = items
            .iter()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .collect();
        match items.as_slice() {
            [] => None,
            [_] | [_, _] => Some(items.join(", ")),
            [a, b, rest @ ..] => Some(format!("{a}, {b} ほか{}件", rest.len())),
        }
    };
    let mut parts: Vec<String> = [list(&rule.hosts), list(&rule.urls)]
        .into_iter()
        .flatten()
        .collect();
    if !rule.source_apps.is_empty() {
        let apps: Vec<_> = rule
            .source_apps
            .iter()
            .map(|b| textures.app_name(b))
            .collect();
        parts.push(format!("{} から", apps.join(", ")));
    }
    parts.join(" · ")
}

fn action_options(
    core: &Core,
    textures: &mut Textures,
    ctx: &egui::Context,
) -> Vec<PopupOption<Action>> {
    std::iter::once(PopupOption {
        value: Action::Picker,
        label: "選択UIを表示".to_owned(),
        icon: None,
    })
    .chain(core.all_choices.iter().map(|choice| PopupOption {
        value: Action::Browser(choice.target.clone()),
        label: choice.label.clone(),
        icon: textures.browser_icon(ctx, &choice.bundle_id),
    }))
    .collect()
}

/// 編集途中の空欄などを取り除いた、保存する形
fn normalize(config: &Config) -> Config {
    let clean = |v: &[String]| {
        v.iter()
            .map(|s| s.trim().to_owned())
            .filter(|s| !s.is_empty())
            .collect()
    };
    Config {
        rules: config
            .rules
            .iter()
            .map(|r| Rule {
                name: r.name.trim().to_owned(),
                hosts: clean(&r.hosts),
                urls: clean(&r.urls),
                source_apps: clean(&r.source_apps),
                action: r.action.clone(),
            })
            .collect(),
        ..config.clone()
    }
}
