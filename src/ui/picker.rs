use eframe::egui::{
    self, Align, Align2, Color32, CornerRadius, Frame, Key, Margin, RichText, Sense, Ui, Vec2,
    text::{LayoutJob, TextFormat},
    vec2,
};
use egui_flex::{Flex, FlexAlign, FlexInstance, FlexJustify, item};

use super::Textures;
use super::theme::{Palette, Typography};
use super::widgets::fitted_text;
use crate::browsers::Choice;

const TILE: Vec2 = vec2(100.0, 104.0);
const TILE_PADDING: Margin = Margin {
    left: 4,
    right: 4,
    top: 10,
    bottom: 6,
};
const GAP: f32 = 2.0;
const PADDING: i8 = 12;
const HEADER_HEIGHT: f32 = 28.0;
const MAX_COLUMNS: usize = 6;
const MIN_WIDTH: f32 = 280.0;
const ICON: f32 = 52.0;

/// ウィンドウは表示前に AppKit 側でサイズを決めるので、中身の大きさをここで見積もる
pub fn window_size(count: usize) -> (f64, f64) {
    let cols = count.clamp(1, MAX_COLUMNS) as f32;
    let rows = count.div_ceil(MAX_COLUMNS).max(1) as f32;
    let padding = PADDING as f32 * 2.0;
    let w = padding + cols * TILE.x + (cols - 1.0) * GAP;
    let h = padding + HEADER_HEIGHT + rows * TILE.y + (rows - 1.0) * GAP;
    (w.max(MIN_WIDTH) as f64, h as f64)
}

#[derive(Default)]
pub struct PickerState {
    selected: usize,
    seen_focus: bool,
}

pub enum Outcome {
    Choose(usize),
    Cancel,
}

pub fn show(
    ui: &mut Ui,
    state: &mut PickerState,
    url: &str,
    choices: &[Choice],
    textures: &mut Textures,
) -> Option<Outcome> {
    let count = choices.len();
    let mut outcome = handle_keys(ui, state, count, count.clamp(1, MAX_COLUMNS));
    let p = Palette::of(ui);
    Frame::new()
        .inner_margin(Margin::same(PADDING))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            Frame::new()
                .inner_margin(Margin {
                    left: 4,
                    right: 4,
                    top: 0,
                    bottom: 8,
                })
                .show(ui, |ui| {
                    ui.add(egui::Label::new(url_line(url, p)).truncate())
                });
            if count == 0 {
                ui.label(RichText::new("ブラウザが見つかりません").color(p.secondary));
                return;
            }
            Flex::horizontal()
                .w_full()
                .wrap(true)
                .justify(FlexJustify::Center)
                .gap(Vec2::splat(GAP))
                .show(ui, |f| {
                    for (n, choice) in choices.iter().enumerate() {
                        let icon = textures.browser_icon(f.ui().ctx(), &choice.bundle_id);
                        let tile = tile(f, choice, icon.as_ref(), n == state.selected, p);
                        if tile.hovered() && f.ui().input(|i| i.pointer.delta() != Vec2::ZERO) {
                            state.selected = n;
                        }
                        if tile.clicked() {
                            outcome = Some(Outcome::Choose(n));
                        }
                        if n < 9 {
                            f.painter().text(
                                tile.rect.right_top() + vec2(-8.0, 6.0),
                                Align2::RIGHT_TOP,
                                (n + 1).to_string(),
                                Typography::Footnote.font(),
                                subdued(p),
                            );
                        }
                    }
                });
        });
    outcome
}

fn handle_keys(ui: &Ui, state: &mut PickerState, count: usize, cols: usize) -> Option<Outcome> {
    let mut outcome = None;
    ui.input(|i| {
        if i.key_pressed(Key::Escape) {
            outcome = Some(Outcome::Cancel);
        }
        if count > 0 {
            let s = state.selected;
            if i.key_pressed(Key::ArrowRight) || i.key_pressed(Key::Tab) {
                state.selected = (s + 1) % count;
            }
            if i.key_pressed(Key::ArrowLeft) {
                state.selected = (s + count - 1) % count;
            }
            if i.key_pressed(Key::ArrowDown) && s + cols < count {
                state.selected = s + cols;
            }
            if i.key_pressed(Key::ArrowUp) && s >= cols {
                state.selected = s - cols;
            }
            if i.key_pressed(Key::Enter) || i.key_pressed(Key::Space) {
                outcome = Some(Outcome::Choose(state.selected));
            }
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
            Some(true) => state.seen_focus = true,
            Some(false) if state.seen_focus => outcome = Some(Outcome::Cancel),
            _ => {}
        }
    });
    outcome
}

fn tile(
    f: &mut FlexInstance,
    choice: &Choice,
    icon: Option<&egui::TextureHandle>,
    selected: bool,
    p: Palette,
) -> egui::Response {
    let frame = Frame::new()
        .fill(if selected {
            p.hover
        } else {
            Color32::TRANSPARENT
        })
        .corner_radius(CornerRadius::same(10))
        .inner_margin(TILE_PADDING);
    let content_width = TILE.x - (TILE_PADDING.left + TILE_PADDING.right) as f32;
    f.add_flex(
        item().frame(frame).min_size(TILE).sense(Sense::click()),
        Flex::vertical()
            .width(content_width)
            .align_items(FlexAlign::Stretch)
            .gap(vec2(0.0, 1.0)),
        |f| {
            f.add_widget(
                item().align_self(FlexAlign::Center),
                super::widgets::icon(icon, ICON),
            );
            f.add_ui(item().min_height(4.0), |_| {});
            let name = RichText::new(&choice.browser_name).text_style(Typography::Callout.style());
            f.add_widget(item(), fitted_text(name.into(), Align::Center));
            if let Some(profile) = &choice.profile_name {
                let profile = RichText::new(profile)
                    .text_style(Typography::Subheadline.style())
                    .color(subdued(p));
                f.add_widget(item(), fitted_text(profile.into(), Align::Center));
            }
        },
    )
    .response
}

/// すりガラスの上では通常の secondary だと沈むので、本文色を薄めて使う
fn subdued(p: Palette) -> Color32 {
    p.text.gamma_multiply(0.62)
}

fn url_line(url: &str, p: Palette) -> LayoutJob {
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    let split = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let (host, path) = rest.split_at(split);
    let mut job = LayoutJob::default();
    job.append(
        host,
        0.0,
        TextFormat::simple(Typography::Headline.font(), p.text),
    );
    job.append(
        path,
        0.0,
        TextFormat::simple(Typography::Callout.font(), p.secondary),
    );
    job
}
