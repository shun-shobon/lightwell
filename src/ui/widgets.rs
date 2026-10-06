use std::sync::Arc;

use eframe::egui::{
    self, Color32, CornerRadius, Frame, Id, Margin, Rect, Response, RichText, Sense, Stroke,
    TextureHandle, Ui, Vec2, pos2, vec2,
};
use egui_flex::{Flex, FlexAlign, FlexInstance, FlexItem, item};

use super::theme::{Palette, Typography};

const GROUP_PADDING: Margin = Margin::symmetric(12, 0);
const ROW_PADDING: Margin = Margin::symmetric(0, 8);
/// コントロールの高さ。行の高さはこれに ROW_PADDING を足したものになる
const CONTROL_HEIGHT: f32 = 22.0;
const DRAG_HANDLE: Vec2 = vec2(14.0, 22.0);

/// 残りの幅をすべて使う flex アイテム。f.add() で置いたウィジェットは幅いっぱいに引き伸ばされる
pub fn fill() -> FlexItem<'static> {
    item().grow(1.0).basis(0.0)
}

/// 残りの幅を使う左寄せのテキスト。収まらない分は省略する
pub fn fill_text(f: &mut FlexInstance, text: impl Into<egui::WidgetText>) {
    f.add_widget(fill(), fitted_text(text.into(), egui::Align::Min));
}

/// 縦に並べる、幅いっぱいで左寄せのテキストの列
pub fn text_column(f: &mut FlexInstance, lines: Vec<egui::WidgetText>) {
    f.add_flex(
        fill(),
        Flex::vertical()
            .align_items(FlexAlign::Stretch)
            .gap(vec2(0.0, 2.0)),
        |f| {
            for line in lines {
                f.add_widget(item(), fitted_text(line, egui::Align::Min));
            }
        },
    );
}

/// 与えられた幅をすべて占め、収まらない分は省略して描くテキスト。
/// Label は文字の大きさだけ確保して中央に置かれるので、flex で引き伸ばしても文字を寄せられない。
/// また最小幅を求めないので、長い文字で flex アイテムが押し広げられることもない
pub fn fitted_text(text: egui::WidgetText, align: egui::Align) -> impl egui::Widget {
    move |ui: &mut Ui| {
        let width = ui.available_width();
        let mut job = Arc::unwrap_or_clone(text.into_layout_job(
            ui.style(),
            egui::FontSelection::Default,
            egui::Align::Center,
        ));
        job.wrap = egui::text::TextWrapping::truncate_at_width(width);
        let galley = ui.fonts_mut(|fonts| fonts.layout_job(job));
        let (rect, mut response) =
            ui.allocate_exact_size(vec2(width, galley.size().y), Sense::hover());
        let x = egui::lerp(
            rect.left()..=rect.right() - galley.size().x,
            align.to_factor(),
        );
        ui.painter()
            .galley(pos2(x, rect.top()), galley, ui.visuals().text_color());
        response.set_intrinsic_size(vec2(0.0, rect.height()));
        response
    }
}

/// macOS のスイッチ
pub fn toggle(ui: &mut Ui, on: &mut bool) -> Response {
    let (rect, mut response) = ui.allocate_exact_size(vec2(32.0, 18.0), Sense::click());
    if response.clicked() {
        *on = !*on;
        response.mark_changed();
    }
    let p = Palette::of(ui);
    let t = ui.ctx().animate_bool_responsive(response.id, *on);
    let off = if ui.visuals().dark_mode {
        Color32::from_white_alpha(40)
    } else {
        Color32::from_black_alpha(28)
    };
    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::same(9), lerp_color(off, p.accent, t));
    let r = rect.height() / 2.0 - 1.5;
    let center = pos2(
        egui::lerp((rect.left() + r + 1.5)..=(rect.right() - r - 1.5), t),
        rect.center().y,
    );
    painter.circle_filled(
        center + vec2(0.0, 0.5),
        r + 0.5,
        Color32::from_black_alpha(40),
    );
    painter.circle_filled(center, r, Color32::WHITE);
    response
}

fn lerp_color(a: Color32, b: Color32, t: f32) -> Color32 {
    let l = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgba_premultiplied(
        l(a.r(), b.r()),
        l(a.g(), b.g()),
        l(a.b(), b.b()),
        l(a.a(), b.a()),
    )
}

/// macOS のチェックボックス
pub fn checkbox(ui: &mut Ui, on: &mut bool) -> Response {
    let (rect, mut response) = ui.allocate_exact_size(Vec2::splat(18.0), Sense::click());
    if response.clicked() {
        *on = !*on;
        response.mark_changed();
    }
    let p = Palette::of(ui);
    let bx = Rect::from_center_size(rect.center(), Vec2::splat(14.0));
    let radius = CornerRadius::same(4);
    let painter = ui.painter();
    if *on {
        painter.rect_filled(bx, radius, p.accent);
        let check = [vec2(3.5, 7.5), vec2(6.0, 10.0), vec2(10.5, 4.0)].map(|v| bx.left_top() + v);
        let stroke = Stroke::new(1.8, Color32::WHITE);
        painter.line_segment([check[0], check[1]], stroke);
        painter.line_segment([check[1], check[2]], stroke);
    } else {
        painter.rect_filled(bx, radius, ui.visuals().text_edit_bg_color());
        painter.rect_stroke(
            bx,
            radius,
            Stroke::new(1.0, p.tertiary),
            egui::StrokeKind::Inside,
        );
    }
    response
}

/// グループの上に付く小見出し
pub fn section_header(ui: &mut Ui, text: &str) {
    Frame::new()
        .inner_margin(Margin {
            left: 12,
            right: 12,
            top: 14,
            bottom: 2,
        })
        .show(ui, |ui| {
            ui.label(RichText::new(text).text_style(Typography::Headline.style()));
        });
}

/// グループの下に付く注釈
pub fn footnote(ui: &mut Ui, text: &str) {
    Frame::new()
        .inner_margin(Margin {
            left: 12,
            right: 12,
            top: 4,
            bottom: 0,
        })
        .show(ui, |ui| {
            ui.label(RichText::new(text).small().color(Palette::of(ui).secondary));
        });
}

/// 角丸の背景に区切り線付きで行を並べる、システム設定風のフォーム
pub fn group<R>(ui: &mut Ui, add_rows: impl FnOnce(&mut Group<'_>) -> R) -> R {
    let p = Palette::of(ui);
    Frame::new()
        .fill(p.group)
        .stroke(Stroke::new(1.0, p.group_stroke))
        .corner_radius(CornerRadius::same(10))
        .inner_margin(GROUP_PADDING)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 0.0;
            add_rows(&mut Group { ui, rows: 0 })
        })
        .inner
}

pub struct Group<'a> {
    ui: &'a mut Ui,
    rows: usize,
}

impl Group<'_> {
    /// 1行を flex で組む。行の矩形を返す
    pub fn row<R>(&mut self, content: impl FnOnce(&mut FlexInstance) -> R) -> (R, Rect) {
        if self.rows > 0 {
            self.ui.add(egui::Separator::default().spacing(0.0));
        }
        self.rows += 1;
        let response = Frame::new().inner_margin(ROW_PADDING).show(self.ui, |ui| {
            Flex::horizontal()
                .w_full()
                .align_items(FlexAlign::Center)
                .align_items_content(egui::Align2::LEFT_CENTER)
                .gap(vec2(8.0, 0.0))
                .show(ui, |f| {
                    // 中身が低い行でも高さを揃える
                    f.add_ui(item().min_height(CONTROL_HEIGHT), |_| {});
                    content(f)
                })
                .inner
        });
        (response.inner, response.response.rect)
    }

    /// 左にラベル、右にコントロール
    pub fn labeled<R>(&mut self, label: &str, control: impl FnOnce(&mut Ui) -> R) -> R {
        self.row(|f| {
            fill_text(f, label);
            f.add_ui(item(), control).inner
        })
        .0
    }

    /// 左にラベルと補足説明、右にコントロール
    pub fn described<R>(
        &mut self,
        label: &str,
        detail: &str,
        control: impl FnOnce(&mut Ui) -> R,
    ) -> R {
        self.row(|f| {
            let detail = secondary(f.ui(), detail).small();
            text_column(f, vec![label.into(), detail.into()]);
            f.add_ui(item(), control).inner
        })
        .0
    }
}

fn secondary(ui: &Ui, text: &str) -> RichText {
    RichText::new(text).color(Palette::of(ui).secondary)
}

/// 色付き角丸の上に白いシンボルを載せたアイコン (システム設定のサイドバー風)
pub fn symbol_badge(ui: &mut Ui, symbol: Option<&TextureHandle>, color: Color32, size: f32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
    ui.painter()
        .rect_filled(rect, CornerRadius::same((size * 0.25) as u8), color);
    if let Some(symbol) = symbol {
        egui::Image::new(symbol)
            .tint(Color32::WHITE)
            .paint_at(ui, rect.shrink(size * 0.2));
    }
}

pub fn icon(icon: Option<&TextureHandle>, size: f32) -> impl egui::Widget + '_ {
    move |ui: &mut Ui| {
        let (rect, response) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
        if let Some(icon) = icon {
            egui::Image::new(icon).paint_at(ui, rect);
        }
        response
    }
}

/// SF Symbols を任意の色で描く
pub fn symbol(symbol: Option<&TextureHandle>, size: f32, tint: Color32) -> impl egui::Widget + '_ {
    move |ui: &mut Ui| {
        let (rect, response) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
        if let Some(symbol) = symbol {
            egui::Image::new(symbol).tint(tint).paint_at(ui, rect);
        }
        response
    }
}

/// 行内の削除ボタンなどに使う、シンボルだけのボタン
pub fn symbol_button(ui: &mut Ui, symbol: Option<&TextureHandle>, tint: Color32) -> Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(22.0), Sense::click());
    if response.hovered() {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(5), Palette::of(ui).hover);
    }
    if let Some(symbol) = symbol {
        egui::Image::new(symbol)
            .tint(tint)
            .paint_at(ui, Rect::from_center_size(rect.center(), Vec2::splat(13.0)));
    }
    response
}

/// 枠なしのアクセントカラーのボタン (「＋ 追加」など)
pub fn link_button(text: &str) -> impl egui::Widget + '_ {
    move |ui: &mut Ui| {
        let accent = Palette::of(ui).accent;
        ui.add(egui::Button::new(RichText::new(text).color(accent)).frame(false))
    }
}

/// 行頭のドラッグハンドルで並べ替えられる、グループ内の行。
/// `row` には行の中身 (ハンドルの右側) を flex で追加する
pub fn reorderable<T>(
    group: &mut Group<'_>,
    id: Id,
    items: &mut Vec<T>,
    mut row: impl FnMut(&mut FlexInstance, usize, &mut T),
) {
    let dragging: Option<usize> = group.ui.data(|d| d.get_temp(id));
    let Palette { accent, hover, .. } = Palette::of(group.ui);
    let mut rects = Vec::with_capacity(items.len());
    for (i, value) in items.iter_mut().enumerate() {
        // 背景は中身より先に描く必要があるが、大きさは中身を置くまで決まらないので、場所だけ確保しておく
        let background = group.ui.painter().add(egui::Shape::Noop);
        let lifted = dragging == Some(i);
        let opacity = group.ui.opacity();
        if lifted {
            group.ui.multiply_opacity(0.45);
        }
        let (handle, rect) = group.row(|f| {
            let handle = f.add_ui(item(), drag_handle).inner;
            row(f, i, value);
            handle
        });
        group.ui.set_opacity(opacity);
        if lifted {
            group.ui.painter().set(
                background,
                egui::epaint::RectShape::filled(
                    rect.expand2(vec2(6.0, 0.0)),
                    CornerRadius::same(6),
                    hover,
                ),
            );
        }
        if handle.drag_started() && dragging.is_none() {
            group.ui.data_mut(|d| d.insert_temp(id, i));
        }
        rects.push(rect);
    }

    let Some(from) = dragging else { return };
    let ui = &mut *group.ui;
    // 挿入位置 (この行の前に入れる)
    let to = ui.ctx().pointer_interact_pos().map(|p| {
        rects
            .iter()
            .position(|r| p.y < r.center().y)
            .unwrap_or(rects.len())
    });
    if let (Some(to), Some(first)) = (to, rects.first()) {
        let y = rects
            .get(to)
            .map_or_else(|| rects[rects.len() - 1].bottom(), |r| r.top());
        ui.painter()
            .hline(first.x_range(), y, Stroke::new(2.0, accent));
    }
    ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
    if ui.input(|i| i.pointer.any_released()) {
        ui.data_mut(|d| d.remove::<usize>(id));
        if let Some(to) = to
            && from < items.len()
        {
            let value = items.remove(from);
            let to = if to > from { to - 1 } else { to };
            items.insert(to.min(items.len()), value);
        }
    }
}

/// 並べ替えられない行を、ドラッグハンドルの行と頭を揃えて置くための空き
pub fn drag_handle_spacer(ui: &mut Ui) {
    ui.allocate_exact_size(DRAG_HANDLE, Sense::hover());
}

fn drag_handle(ui: &mut Ui) -> Response {
    let (rect, response) = ui.allocate_exact_size(DRAG_HANDLE, Sense::drag());
    let p = Palette::of(ui);
    let color = if response.hovered() {
        p.secondary
    } else {
        p.tertiary
    };
    for dx in [-2.5, 2.5] {
        for dy in [-5.0, 0.0, 5.0] {
            ui.painter()
                .circle_filled(rect.center() + vec2(dx, dy), 1.3, color);
        }
    }
    if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
    }
    response
}

/// アイコン付きの選択肢から1つ選ぶポップアップボタン
pub struct PopupOption<T> {
    pub value: T,
    pub label: String,
    pub icon: Option<TextureHandle>,
}

/// `indicator` は右端に出す上下矢印 (SF Symbols の chevron.up.chevron.down)
pub fn popup<T: PartialEq + Clone>(
    ui: &mut Ui,
    current: &mut T,
    options: &[PopupOption<T>],
    fallback_label: &str,
    indicator: Option<&TextureHandle>,
) {
    let selected = options.iter().find(|o| o.value == *current);
    let mut atoms = egui::Atoms::default();
    if let Some(icon) = selected.and_then(|o| o.icon.as_ref()) {
        atoms.push_right(egui::Image::new(icon).fit_to_exact_size(Vec2::splat(16.0)));
    }
    atoms.push_right(selected.map_or(fallback_label, |o| o.label.as_str()));
    if let Some(indicator) = indicator {
        let tint = Palette::of(ui).secondary;
        atoms.push_right(
            egui::Image::new(indicator)
                .fit_to_exact_size(vec2(9.0, 11.0))
                .tint(tint),
        );
    }
    ui.menu_button(atoms, |ui| {
        ui.set_min_width(220.0);
        for option in options {
            let mut atoms = egui::Atoms::default();
            if let Some(icon) = &option.icon {
                atoms.push_right(egui::Image::new(icon).fit_to_exact_size(Vec2::splat(16.0)));
            }
            atoms.push_right(option.label.as_str());
            if ui
                .add(egui::Button::selectable(option.value == *current, atoms))
                .clicked()
            {
                *current = option.value.clone();
                ui.close();
            }
        }
    });
}
