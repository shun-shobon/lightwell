use dioxus::prelude::*;

use super::icons::Icon;
use crate::browsers::Choice;

const TILE_WIDTH: usize = 100;
const TILE_HEIGHT: usize = 104;
const GAP: usize = 2;
const PADDING: usize = 12;
const URL_HEIGHT: usize = 28;
const MAX_COLUMNS: usize = 6;
const MIN_WIDTH: usize = 280;

/// ウィンドウは表示前に AppKit 側でサイズを決めるので、中身の大きさをここで見積もる
pub fn window_size(count: usize) -> (f64, f64) {
    let cols = count.clamp(1, MAX_COLUMNS);
    let rows = count.div_ceil(MAX_COLUMNS).max(1);
    let w = PADDING * 2 + cols * TILE_WIDTH + (cols - 1) * GAP;
    let h = PADDING * 2 + URL_HEIGHT + rows * TILE_HEIGHT + (rows - 1) * GAP;
    (w.max(MIN_WIDTH) as f64, h as f64)
}

#[derive(Clone, PartialEq)]
pub struct PickerRequest {
    pub url: String,
    pub choices: Vec<Choice>,
}

/// 選んだ選択肢の番号、キャンセルなら None で `on_done` を呼ぶ
#[component]
pub fn Picker(request: PickerRequest, on_done: EventHandler<Option<usize>>) -> Element {
    let mut selected = use_signal(|| 0);
    let count = request.choices.len();
    let cols = count.clamp(1, MAX_COLUMNS);

    let onkeydown = move |e: KeyboardEvent| {
        let s = selected();
        match e.key() {
            Key::Escape => on_done(None),
            _ if count == 0 => return,
            Key::Enter => on_done(Some(s)),
            Key::Character(c) if c == " " => on_done(Some(s)),
            Key::ArrowRight | Key::Tab => selected.set((s + 1) % count),
            Key::ArrowLeft => selected.set((s + count - 1) % count),
            Key::ArrowDown if s + cols < count => selected.set(s + cols),
            Key::ArrowUp if s >= cols => selected.set(s - cols),
            Key::Character(c) => match c.parse::<usize>() {
                Ok(n @ 1..=9) if n <= count => on_done(Some(n - 1)),
                _ => return,
            },
            _ => return,
        }
        e.prevent_default();
    };

    let (host, path) = split_url(&request.url);
    rsx! {
        div {
            class: "picker",
            style: "padding: {PADDING}px",
            tabindex: 0,
            onmounted: move |e| async move { _ = e.set_focus(true).await },
            onkeydown,
            div { class: "picker-url", style: "height: {URL_HEIGHT}px",
                span { class: "host", "{host}" }
                span { class: "path", "{path}" }
            }
            if count == 0 {
                p { class: "secondary", "ブラウザが見つかりません" }
            }
            div {
                class: "tiles",
                style: "grid-template-columns: repeat({cols}, {TILE_WIDTH}px); grid-auto-rows: {TILE_HEIGHT}px; gap: {GAP}px",
                for (n, choice) in request.choices.into_iter().enumerate() {
                    div {
                        class: if n == selected() { "tile selected" } else { "tile" },
                        onmousemove: move |_| selected.set(n),
                        onclick: move |_| on_done(Some(n)),
                        Icon { src: choice.icon, size: 52 }
                        span { class: "name", "{choice.browser_name}" }
                        if let Some(profile) = choice.profile_name {
                            span { class: "profile", "{profile}" }
                        }
                        if n < 9 {
                            span { class: "key", "{n + 1}" }
                        }
                    }
                }
            }
        }
    }
}

fn split_url(url: &str) -> (&str, &str) {
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    rest.split_at(rest.find(['/', '?', '#']).unwrap_or(rest.len()))
}
