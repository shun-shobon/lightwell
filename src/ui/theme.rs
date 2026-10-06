use std::sync::Arc;

use eframe::egui::{
    self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Margin, Shadow,
    Stroke, TextStyle, Theme, Visuals, epaint::text::VariationCoords,
};

use crate::macos::{self, FontFace};

const SEMIBOLD: &str = "semibold";

#[derive(Clone, Copy)]
pub enum Typography {
    Title,
    Headline,
    Body,
    Callout,
    Subheadline,
    Footnote,
}

impl Typography {
    const ALL: [Self; 6] = [
        Self::Title,
        Self::Headline,
        Self::Body,
        Self::Callout,
        Self::Subheadline,
        Self::Footnote,
    ];

    pub fn font(self) -> FontId {
        let (size, semibold) = match self {
            Self::Title => (15.0, true),
            Self::Headline => (13.0, true),
            Self::Body => (13.0, false),
            Self::Callout => (12.0, false),
            Self::Subheadline => (11.0, false),
            Self::Footnote => (10.0, false),
        };
        let family = if semibold {
            FontFamily::Name(SEMIBOLD.into())
        } else {
            FontFamily::Proportional
        };
        FontId::new(size, family)
    }

    pub fn style(self) -> TextStyle {
        TextStyle::Name(self.name().into())
    }

    fn name(self) -> &'static str {
        match self {
            Self::Title => "title",
            Self::Headline => "headline",
            Self::Body => "body",
            Self::Callout => "callout",
            Self::Subheadline => "subheadline",
            Self::Footnote => "footnote",
        }
    }
}

#[derive(Clone, Copy)]
pub struct Palette {
    pub window: Color32,
    pub group: Color32,
    pub group_stroke: Color32,
    pub separator: Color32,
    pub text: Color32,
    pub secondary: Color32,
    pub tertiary: Color32,
    pub hover: Color32,
    pub accent: Color32,
    pub destructive: Color32,
}

impl Palette {
    pub fn of(ui: &egui::Ui) -> Self {
        let accent = ui.visuals().selection.stroke.color;
        if ui.visuals().dark_mode {
            Self::dark(accent)
        } else {
            Self::light(accent)
        }
    }

    fn light(accent: Color32) -> Self {
        Self {
            window: Color32::from_rgb(242, 242, 242),
            group: Color32::from_rgb(252, 252, 252),
            group_stroke: Color32::from_black_alpha(14),
            separator: Color32::from_black_alpha(20),
            text: Color32::from_rgb(29, 29, 31),
            secondary: Color32::from_rgb(110, 110, 115),
            tertiary: Color32::from_rgb(160, 160, 165),
            hover: Color32::from_black_alpha(18),
            accent,
            destructive: Color32::from_rgb(255, 59, 48),
        }
    }

    fn dark(accent: Color32) -> Self {
        Self {
            window: Color32::from_rgb(30, 30, 30),
            group: Color32::from_rgb(40, 40, 40),
            group_stroke: Color32::from_white_alpha(14),
            separator: Color32::from_white_alpha(18),
            text: Color32::from_rgb(235, 235, 240),
            secondary: Color32::from_rgb(152, 152, 157),
            tertiary: Color32::from_rgb(100, 100, 105),
            hover: Color32::from_white_alpha(22),
            accent,
            destructive: Color32::from_rgb(255, 69, 58),
        }
    }
}

pub fn install(ctx: &egui::Context, accent: [u8; 3]) {
    install_fonts(ctx);
    // egui_flex は前のフレームの大きさで配置するので、画面を切り替えた直後は測り直しの再描画が要る。
    // 既定の2回では入れ子の flex が落ち着かず、測る前の配置が一瞬見えてしまう
    ctx.options_mut(|o| o.max_passes = std::num::NonZeroUsize::new(6).unwrap());
    let accent = Color32::from_rgb(accent[0], accent[1], accent[2]);
    for theme in [Theme::Light, Theme::Dark] {
        ctx.style_mut_of(theme, |style| {
            let palette = match theme {
                Theme::Light => Palette::light(accent),
                Theme::Dark => Palette::dark(accent),
            };
            style.visuals = visuals(theme, palette);
            style.text_styles = [
                (TextStyle::Small, Typography::Subheadline.font()),
                (TextStyle::Body, Typography::Body.font()),
                (TextStyle::Button, Typography::Body.font()),
                (TextStyle::Heading, Typography::Title.font()),
                (TextStyle::Monospace, Typography::Body.font()),
            ]
            .into_iter()
            .chain(Typography::ALL.map(|t| (t.style(), t.font())))
            .collect();
            let spacing = &mut style.spacing;
            spacing.item_spacing = egui::vec2(8.0, 6.0);
            spacing.button_padding = egui::vec2(10.0, 3.0);
            spacing.interact_size = egui::vec2(24.0, 22.0);
            spacing.menu_margin = Margin::same(5);
            spacing.combo_width = 200.0;
            spacing.icon_spacing = 6.0;
            style.interaction.selectable_labels = false;
        });
    }
}

fn visuals(theme: Theme, p: Palette) -> Visuals {
    let dark = theme == Theme::Dark;
    let mut v = if dark {
        Visuals::dark()
    } else {
        Visuals::light()
    };
    v.override_text_color = Some(p.text);
    v.weak_text_color = Some(p.secondary);
    v.hyperlink_color = p.accent;
    v.selection.bg_fill = p.accent.gamma_multiply(0.45);
    // Palette::of が参照するため、アクセントカラーはここに入れておく
    v.selection.stroke = Stroke::new(1.0, p.accent);
    v.panel_fill = p.window;
    v.window_fill = if dark {
        Color32::from_rgb(44, 44, 46)
    } else {
        Color32::from_rgb(250, 250, 250)
    };
    v.window_stroke = Stroke::new(1.0, p.group_stroke);
    v.window_corner_radius = CornerRadius::same(10);
    v.menu_corner_radius = CornerRadius::same(8);
    v.window_shadow = Shadow {
        offset: [0, 6],
        blur: 24,
        spread: 0,
        color: Color32::from_black_alpha(if dark { 110 } else { 50 }),
    };
    v.popup_shadow = v.window_shadow;
    v.extreme_bg_color = if dark {
        Color32::from_rgb(28, 28, 30)
    } else {
        Color32::WHITE
    };
    v.text_edit_bg_color = Some(if dark {
        Color32::from_rgb(50, 50, 52)
    } else {
        Color32::WHITE
    });
    v.faint_bg_color = p.hover;

    let control = if dark {
        Color32::from_rgb(78, 78, 82)
    } else {
        Color32::WHITE
    };
    let control_hover = if dark {
        Color32::from_rgb(90, 90, 94)
    } else {
        Color32::from_rgb(246, 246, 246)
    };
    let control_active = if dark {
        Color32::from_rgb(105, 105, 110)
    } else {
        Color32::from_rgb(230, 230, 230)
    };
    let control_stroke = if dark {
        Stroke::new(0.5, Color32::from_white_alpha(20))
    } else {
        Stroke::new(0.5, Color32::from_black_alpha(45))
    };
    let radius = CornerRadius::same(6);

    let w = &mut v.widgets;
    w.noninteractive.bg_stroke = Stroke::new(1.0, p.separator);
    w.noninteractive.fg_stroke = Stroke::new(1.0, p.text);
    w.noninteractive.corner_radius = radius;
    for (widget, fill) in [
        (&mut w.inactive, control),
        (&mut w.hovered, control_hover),
        (&mut w.active, control_active),
        (&mut w.open, control_active),
    ] {
        widget.bg_fill = fill;
        widget.weak_bg_fill = fill;
        widget.bg_stroke = control_stroke;
        widget.fg_stroke = Stroke::new(1.0, p.text);
        widget.corner_radius = radius;
        widget.expansion = 0.0;
    }
    w.active.bg_stroke = Stroke::new(1.5, p.accent.gamma_multiply(0.8));
    v
}

fn install_fonts(ctx: &egui::Context) {
    let system = macos::ui_fonts(Typography::Body.font().size.into());
    let mut fonts = FontDefinitions::default();
    let builtin = fonts
        .families
        .get(&FontFamily::Proportional)
        .cloned()
        .unwrap_or_default();

    let mut family = |name: &str, latin: Option<&FontFace>, japanese: Option<&FontFace>| {
        let mut chain = Vec::new();
        if let Some(latin) = latin {
            fonts
                .font_data
                .insert(name.to_owned(), font_data(latin, 0.0));
            chain.push(name.to_owned());
        }
        if let Some(japanese) = japanese {
            let offset = latin.map_or(0.0, |latin| baseline_offset(latin, japanese));
            let key = format!("{name}-ja");
            fonts
                .font_data
                .insert(key.clone(), font_data(japanese, offset));
            chain.push(key);
        }
        // システムフォントが読めなかったときのため、egui の同梱フォントを末尾に残す
        chain.extend(builtin.iter().cloned());
        chain
    };
    let regular = family("system", system.regular.as_ref(), system.japanese.as_ref());
    let semibold = family(
        "system-semibold",
        system.semibold.as_ref(),
        system.japanese_semibold.as_ref(),
    );
    // コードを表示する箇所はないので、等幅も本文と同じフォントにする
    fonts
        .families
        .insert(FontFamily::Monospace, regular.clone());
    fonts.families.insert(FontFamily::Proportional, regular);
    fonts
        .families
        .insert(FontFamily::Name(SEMIBOLD.into()), semibold);
    ctx.set_fonts(fonts);
}

fn font_data(face: &FontFace, y_offset_factor: f32) -> Arc<FontData> {
    let mut data = FontData::from_owned(face.bytes.clone());
    data.index = face.index;
    data.tweak.coords = VariationCoords::new(face.variation.iter().map(|(tag, v)| (tag, *v)));
    data.tweak.y_offset_factor = y_offset_factor;
    // macOS はヒンティングしないので、合わせないと文字の形が潰れて見える
    data.tweak.hinting = Some(false);
    Arc::new(data)
}

/// egui は代替フォントの字形を「行の高さの差の半分」ずらして縦中央に置くため、
/// ascent/descent の配分が違うフォントを混ぜるとベースラインがずれる。
/// 主フォントのベースラインに揃えるための補正量を、文字サイズに対する比で返す
fn baseline_offset(primary: &FontFace, fallback: &FontFace) -> f32 {
    let metrics = |face: &FontFace| {
        let font = skrifa::FontRef::from_index(&face.bytes, face.index).ok()?;
        // egui (epaint) と同じ取り方をする
        let m = skrifa::MetadataProvider::metrics(
            &font,
            skrifa::instance::Size::unscaled(),
            skrifa::instance::LocationRef::default(),
        );
        let em = m.units_per_em as f32;
        let ascent = m.ascent / em;
        Some((ascent, ascent - m.descent / em + m.leading / em))
    };
    let (Some((p_ascent, p_height)), Some((f_ascent, f_height))) =
        (metrics(primary), metrics(fallback))
    else {
        return 0.0;
    };
    p_ascent - f_ascent - 0.5 * (p_height - f_height)
}
