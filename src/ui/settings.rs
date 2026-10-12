use dioxus::prelude::*;
use objc2::MainThreadMarker;

use super::icons::{self, Icon, Symbol};
use crate::app::Core;
use crate::browsers;
use crate::config::{Action, Config, Rule, Target};
use crate::macos::{self, LoginItemStatus};

pub const SIZE: (f64, f64) = (760.0, 540.0);
pub const MIN_SIZE: (f64, f64) = (640.0, 420.0);

#[derive(Clone, Copy, PartialEq)]
enum Page {
    General,
    Picker,
    Rules,
    Rule(usize),
}

const PAGES: [(Page, &str, &str); 3] = [
    (Page::General, "一般", "gearshape"),
    (Page::Picker, "選択UI", "square.grid.2x2"),
    (Page::Rules, "ルール", "arrow.triangle.branch"),
];

#[component]
pub fn Settings() -> Element {
    let mut core = use_context::<Signal<Core>>();
    let draft = use_signal(|| core.peek().store.config().clone());
    let mut page = use_signal(initial_page);
    let mut error = use_signal(|| None::<String>);

    use_effect(move || {
        let config = normalize(&draft.read());
        if config != *core.peek().store.config() {
            let result = core.write().store.save(config);
            error.set(result.err().map(|e| format!("保存に失敗しました: {e}")));
        }
    });

    let title = match page() {
        Page::General => "一般".to_owned(),
        Page::Picker => "選択UI".to_owned(),
        Page::Rules => "ルール".to_owned(),
        Page::Rule(i) => draft
            .read()
            .rules
            .get(i)
            .map_or("", display_name)
            .to_owned(),
    };
    rsx! {
        div { class: "settings",
            nav { class: "sidebar",
                div { class: "titlebar", onmousedown: drag_window }
                for (target, label, symbol) in PAGES {
                    div {
                        class: if is_in(page(), target) { "nav selected" } else { "nav" },
                        onclick: move |_| page.set(target),
                        Symbol { name: symbol, size: 15 }
                        "{label}"
                    }
                }
            }
            main { class: "content",
                header { class: "titlebar", onmousedown: drag_window,
                    if let Page::Rule(_) = page() {
                        button {
                            class: "icon-button",
                            onmousedown: |e| e.stop_propagation(),
                            onclick: move |_| page.set(Page::Rules),
                            Symbol { name: "chevron.left", size: 13 }
                        }
                    }
                    h1 { "{title}" }
                }
                div { class: "page",
                    match page() {
                        Page::General => rsx! { GeneralPage { draft, error } },
                        Page::Picker => rsx! { PickerPage { draft } },
                        Page::Rules => rsx! { RulesPage { draft, page } },
                        Page::Rule(index) => rsx! { RulePage { draft, page, index } },
                    }
                    if let Some(err) = error() {
                        p { class: "error", "{err}" }
                    }
                }
            }
        }
    }
}

#[component]
fn GeneralPage(draft: Signal<Config>, error: Signal<Option<String>>) -> Element {
    let core = use_context::<Signal<Core>>();
    let is_default = use_signal(macos::is_default_browser);
    let mut login_item = use_signal(macos::login_item_status);
    let core = core.read();
    let actions = action_options(&core);
    let alternatives = std::iter::once((None, "使わない".to_owned()))
        .chain(
            actions
                .iter()
                .map(|(a, label)| (Some(a.clone()), label.clone())),
        )
        .collect();
    let default_detail = if is_default() {
        "リンクは Lightwell が受け取って振り分けます"
    } else {
        "リンクを振り分けるには、Lightwell をデフォルトブラウザにします"
    };
    let requires_approval = login_item() == LoginItemStatus::RequiresApproval;
    let login_detail = if requires_approval {
        "システム設定のログイン項目で許可してください"
    } else {
        "常駐させておくと、リンクを開いたときに待たされません"
    };
    let path = core.store.path().to_owned();
    let path_label = path.display().to_string();
    rsx! {
        div { class: "group",
            Row {
                label: "デフォルトブラウザ",
                detail: default_detail,
                if is_default() {
                    span { class: "secondary", "設定済み" }
                } else {
                    button { class: "primary", onclick: |_| macos::set_as_default_browser(), "デフォルトにする" }
                }
            }
            Row {
                label: "ログイン時に起動",
                detail: login_detail,
                if requires_approval {
                    button { onclick: |_| macos::open_login_items_settings(), "開く" }
                }
                input {
                    r#type: "checkbox",
                    class: "toggle",
                    checked: login_item() != LoginItemStatus::Disabled,
                    onchange: move |e| {
                        if let Err(err) = macos::set_login_item(e.checked()) {
                            error.set(Some(format!("ログイン項目の変更に失敗しました: {err}")));
                        }
                        login_item.set(macos::login_item_status());
                    },
                }
            }
        }

        h2 { "リンクの開き方" }
        div { class: "group",
            Row { label: "通常", detail: "どのルールにも一致しないとき",
                Select {
                    value: draft.read().default.clone(),
                    options: actions,
                    onchange: move |action: Action| draft.write().default = action,
                }
            }
            Row { label: "Fn キーを押しながら", detail: "ルールより優先されます",
                Select {
                    value: draft.read().alternative.clone(),
                    options: alternatives,
                    onchange: move |action: Option<Action>| draft.write().alternative = action,
                }
            }
        }

        h2 { "設定ファイル" }
        div { class: "group",
            div { class: "row",
                span { class: "grow truncate secondary", "{path_label}" }
                button { onclick: move |_| macos::reveal_in_finder(&path), "Finder で表示" }
            }
        }
        p { class: "footnote",
            "変更は自動で保存されます。ファイルを直接編集した内容も、次にリンクを開いたときに反映されます。"
        }
        if let Some(err) = core.store.error() {
            p { class: "error",
                "設定ファイルを読み込めませんでした。直前の設定を使っています。\n{err}"
            }
        }
    }
}

#[component]
fn PickerPage(draft: Signal<Config>) -> Element {
    let core = use_context::<Signal<Core>>();
    let mut dragging = use_signal(|| None::<usize>);
    let c = core.read();
    let (shown, hidden) =
        browsers::picker_partition(&c.all_choices, draft.read().picker_items.as_deref());
    rsx! {
        div { class: "group", ondragover: |e| e.prevent_default(),
            for (i, target) in shown.iter().enumerate() {
                div {
                    class: if dragging() == Some(i) { "row dragging" } else { "row" },
                    draggable: true,
                    ondragstart: move |_| dragging.set(Some(i)),
                    ondragend: move |_| dragging.set(None),
                    ondragenter: move |_| {
                        if let Some(from) = dragging() && from != i {
                            edit_shown(core, draft, |items| move_item(items, from, i));
                            dragging.set(Some(i));
                        }
                    },
                    span { class: "handle", Symbol { name: "line.3.horizontal", size: 12 } }
                    input {
                        r#type: "checkbox",
                        checked: true,
                        onchange: move |_| edit_shown(core, draft, |items| _ = items.remove(i)),
                    }
                    BrowserLabel { target: target.clone() }
                    if !c.browsers.iter().any(|b| b.bundle_id == target.bundle_id) {
                        span { class: "detail", "未インストール" }
                    }
                    if i < 9 {
                        span { class: "tertiary", "{i + 1}" }
                    }
                }
            }
            for target in hidden {
                div { class: "row",
                    span { class: "handle" }
                    input {
                        r#type: "checkbox",
                        checked: false,
                        onchange: move |_| edit_shown(core, draft, |items| items.push(target.clone())),
                    }
                    BrowserLabel { target: target.clone(), dimmed: true }
                }
            }
        }
        p { class: "footnote",
            "チェックしたものが、この順番で選択UIに並びます。ドラッグで並べ替えられます。先頭9件は数字キーで選べます。"
        }
    }
}

/// 未設定 (検出したものをすべて表示) の状態から操作したときは、その時点の表示内容を明示的な設定にする
fn edit_shown(core: Signal<Core>, mut draft: Signal<Config>, edit: impl FnOnce(&mut Vec<Target>)) {
    let (mut shown, _) = browsers::picker_partition(
        &core.read().all_choices,
        draft.read().picker_items.as_deref(),
    );
    edit(&mut shown);
    draft.write().picker_items = Some(shown);
}

#[component]
fn BrowserLabel(target: Target, #[props(default)] dimmed: bool) -> Element {
    let core = use_context::<Signal<Core>>();
    let core = core.read();
    rsx! {
        Icon { src: icons::browser_icon(&core.browsers, &target.bundle_id), size: 22 }
        span { class: if dimmed { "grow truncate secondary" } else { "grow truncate" },
            "{browsers::describe_target(&core.browsers, &target)}"
        }
    }
}

#[component]
fn RulesPage(draft: Signal<Config>, page: Signal<Page>) -> Element {
    let core = use_context::<Signal<Core>>();
    let mut dragging = use_signal(|| None::<usize>);
    let core = core.read();
    let rules = draft.read().rules.clone();
    rsx! {
        div { class: "group", ondragover: |e| e.prevent_default(),
            if rules.is_empty() {
                div { class: "row secondary", "ルールはまだありません" }
            }
            for (i, rule) in rules.iter().enumerate() {
                div {
                    class: if dragging() == Some(i) { "row clickable dragging" } else { "row clickable" },
                    draggable: true,
                    ondragstart: move |_| dragging.set(Some(i)),
                    ondragend: move |_| dragging.set(None),
                    ondragenter: move |_| {
                        if let Some(from) = dragging() && from != i {
                            move_item(&mut draft.write().rules, from, i);
                            dragging.set(Some(i));
                        }
                    },
                    onclick: move |_| page.set(Page::Rule(i)),
                    span { class: "handle", Symbol { name: "line.3.horizontal", size: 12 } }
                    div { class: "grow",
                        div { class: if rule.name.trim().is_empty() { "truncate secondary" } else { "truncate" },
                            "{display_name(rule)}"
                        }
                        if rule.has_conditions() {
                            div { class: "detail truncate", "{summarize(rule)}" }
                        } else {
                            div { class: "detail warning", "条件がないため使われません" }
                        }
                    }
                    if let Action::Browser(target) = &rule.action {
                        Icon { src: icons::browser_icon(&core.browsers, &target.bundle_id), size: 18 }
                    }
                    span { class: "secondary", "{browsers::describe(&core.browsers, &rule.action)}" }
                    Symbol { name: "chevron.right", size: 10 }
                }
            }
        }
        button {
            class: "add",
            onclick: move |_| {
                let mut config = draft.write();
                config.rules.push(Rule {
                    name: "新しいルール".to_owned(),
                    ..Default::default()
                });
                page.set(Page::Rule(config.rules.len() - 1));
            },
            "＋ ルールを追加"
        }
        p { class: "footnote",
            "上から順に評価し、最初に一致したルールのブラウザで開きます。ドラッグで並べ替えられます。"
        }
    }
}

#[component]
fn RulePage(draft: Signal<Config>, page: Signal<Page>, index: usize) -> Element {
    let core = use_context::<Signal<Core>>();
    let running_apps = use_hook(macos::running_apps);
    let Some(rule) = draft.read().rules.get(index).cloned() else {
        return rsx! {};
    };
    let actions = action_options(&core.read());
    let i = index;
    rsx! {
        div { class: "group",
            Row { label: "名前",
                input {
                    r#type: "text",
                    class: "name",
                    value: "{rule.name}",
                    oninput: move |e| draft.write().rules[i].name = e.value(),
                }
            }
        }

        PatternList {
            title: "ホスト",
            placeholder: "*.example.com",
            caption: "* は任意の文字列に一致します。*.example.com は example.com 自体には一致しません",
            add_label: "＋ ホストを追加",
            values: rule.hosts.clone(),
            onchange: move |hosts| draft.write().rules[i].hosts = hosts,
        }
        PatternList {
            title: "URL",
            placeholder: "https://github.com/my-org/*",
            caption: "URL全体に対するパターンです",
            add_label: "＋ URLを追加",
            values: rule.urls.clone(),
            onchange: move |urls| draft.write().rules[i].urls = urls,
        }

        h2 { "開き元アプリ" }
        div { class: "group",
            for (j, bundle_id) in rule.source_apps.iter().enumerate() {
                div { class: "row",
                    Icon { src: icons::app_icon(bundle_id), size: 22 }
                    span { "{app_name(bundle_id)}" }
                    span { class: "grow truncate detail", "{bundle_id}" }
                    RemoveButton { onclick: move |_| _ = draft.write().rules[i].source_apps.remove(j) }
                }
            }
            div { class: "row",
                select {
                    id: "add-source-app",
                    class: "link",
                    onchange: move |e| {
                        let bundle_id = match e.value().as_str() {
                            "" => None,
                            "other" => MainThreadMarker::new().and_then(macos::choose_app),
                            id => Some(id.to_owned()),
                        };
                        let apps = &mut draft.write().rules[i].source_apps;
                        if let Some(id) = bundle_id && !apps.contains(&id) {
                            apps.push(id);
                        }
                        // 選んだ項目が表示に残らないよう見出しに戻す。value は変わらないので再描画では戻らない
                        dioxus::document::eval("document.getElementById('add-source-app').value = ''");
                    },
                    option { value: "", "＋ アプリを追加" }
                    optgroup { label: "起動中のアプリ",
                        for app in running_apps.iter().filter(|a| !rule.source_apps.contains(&a.bundle_id)) {
                            option { value: "{app.bundle_id}", "{app.name}" }
                        }
                    }
                    option { value: "other", "その他のアプリ…" }
                }
            }
        }
        p { class: "footnote", "どのアプリでリンクをクリックしたかで振り分けます" }

        h2 { "開くブラウザ" }
        div { class: "group",
            Row { label: "ブラウザ",
                Select {
                    value: rule.action.clone(),
                    options: actions,
                    onchange: move |action: Action| draft.write().rules[i].action = action,
                }
            }
        }
        if !rule.has_conditions() {
            p { class: "warning",
                "ホスト・URL・開き元アプリのいずれかを設定するまで、このルールは使われません"
            }
        }

        button {
            class: "destructive",
            onclick: move |_| {
                draft.write().rules.remove(i);
                page.set(Page::Rules);
            },
            "ルールを削除"
        }
    }
}

/// 入力中の行を残すため、空文字列も含めたまま編集する。保存時に normalize で取り除く
#[component]
fn PatternList(
    title: &'static str,
    placeholder: &'static str,
    caption: &'static str,
    add_label: &'static str,
    values: Vec<String>,
    onchange: EventHandler<Vec<String>>,
) -> Element {
    let mut added = use_signal(|| false);
    let edit = move |values: &Vec<String>, f: &dyn Fn(&mut Vec<String>)| {
        let mut values = values.clone();
        f(&mut values);
        onchange(values);
    };
    rsx! {
        h2 { "{title}" }
        div { class: "group",
            for (j, value) in values.iter().enumerate() {
                div { class: "row",
                    input {
                        r#type: "text",
                        class: "grow",
                        placeholder,
                        value: "{value}",
                        onmounted: move |e| async move {
                            if added() {
                                _ = e.set_focus(true).await;
                            }
                        },
                        oninput: {
                            let values = values.clone();
                            move |e: FormEvent| edit(&values, &|v| v[j] = e.value())
                        },
                    }
                    RemoveButton {
                        onclick: {
                            let values = values.clone();
                            move |_| edit(&values, &|v| _ = v.remove(j))
                        },
                    }
                }
            }
            div { class: "row",
                button {
                    class: "link",
                    onclick: {
                        let values = values.clone();
                        move |_| {
                            added.set(true);
                            edit(&values, &|v| v.push(String::new()));
                        }
                    },
                    "{add_label}"
                }
            }
        }
        p { class: "footnote", "{caption}" }
    }
}

#[component]
fn Row(label: &'static str, #[props(default)] detail: &'static str, children: Element) -> Element {
    rsx! {
        div { class: "row",
            div { class: "grow",
                div { class: "truncate", "{label}" }
                if !detail.is_empty() {
                    div { class: "detail", "{detail}" }
                }
            }
            {children}
        }
    }
}

#[component]
fn RemoveButton(onclick: EventHandler<MouseEvent>) -> Element {
    rsx! {
        button { class: "icon-button tertiary", onclick: move |e| onclick.call(e),
            Symbol { name: "minus.circle.fill", size: 13 }
        }
    }
}

/// 一覧にない値 (アンインストールしたブラウザなど) は、選べない項目として表示する
#[component]
fn Select<T: Clone + PartialEq + 'static>(
    value: T,
    options: Vec<(T, String)>,
    onchange: EventHandler<T>,
) -> Element {
    let selected = options.iter().position(|(v, _)| *v == value);
    let labels: Vec<String> = options.iter().map(|(_, label)| label.clone()).collect();
    rsx! {
        select {
            onchange: move |e| {
                if let Some((value, _)) = e.value().parse::<usize>().ok().and_then(|i| options.get(i)) {
                    onchange(value.clone());
                }
            },
            if selected.is_none() {
                option { disabled: true, selected: true, "不明なブラウザ" }
            }
            for (i, label) in labels.iter().enumerate() {
                option { value: "{i}", selected: selected == Some(i), "{label}" }
            }
        }
    }
}

fn drag_window(_: MouseEvent) {
    dioxus::desktop::window().drag();
}

fn is_in(current: Page, target: Page) -> bool {
    match current {
        Page::Rule(_) => target == Page::Rules,
        current => current == target,
    }
}

fn move_item<T>(items: &mut Vec<T>, from: usize, to: usize) {
    let item = items.remove(from);
    items.insert(to, item);
}

fn display_name(rule: &Rule) -> &str {
    match rule.name.trim() {
        "" => "名称未設定",
        name => name,
    }
}

fn app_name(bundle_id: &str) -> String {
    macos::app_name(bundle_id).unwrap_or_else(|| bundle_id.to_owned())
}

fn summarize(rule: &Rule) -> String {
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
        let apps: Vec<_> = rule.source_apps.iter().map(|b| app_name(b)).collect();
        parts.push(format!("{} から", apps.join(", ")));
    }
    parts.join(" · ")
}

fn action_options(core: &Core) -> Vec<(Action, String)> {
    std::iter::once((Action::Picker, "選択UIを表示".to_owned()))
        .chain(
            core.all_choices
                .iter()
                .map(|choice| (Action::Browser(choice.target.clone()), choice.label.clone())),
        )
        .collect()
}

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

fn initial_page() -> Page {
    #[cfg(debug_assertions)]
    match std::env::var("LIGHTWELL_PREVIEW").as_deref() {
        Ok("settings:picker") => return Page::Picker,
        Ok("settings:rules") => return Page::Rules,
        Ok("settings:rule") => return Page::Rule(0),
        _ => {}
    }
    Page::General
}
