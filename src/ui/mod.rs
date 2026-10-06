pub mod picker;
pub mod settings;
pub mod theme;
pub mod widgets;

use std::collections::HashMap;

use eframe::egui::{self, ColorImage, TextureFilter, TextureHandle, TextureOptions};

use crate::browsers::{Browser, ICON_SIZE};
use crate::macos;

const SMALL_ICON_SIZE: usize = 64;
const SYMBOL_SIZE: usize = 64;

/// ラスタライズは数ミリ秒かかるので、表示のたびに作り直さない
#[derive(Default)]
pub struct Textures {
    browsers_rev: u64,
    browser_icons: HashMap<String, TextureHandle>,
    app_icons: HashMap<String, Option<TextureHandle>>,
    app_names: HashMap<String, String>,
    symbols: HashMap<&'static str, Option<TextureHandle>>,
}

fn options() -> TextureOptions {
    // 縮小表示が多いので、ミップマップなしだとギザギザになる
    TextureOptions::LINEAR.with_mipmap_mode(Some(TextureFilter::Linear))
}

fn texture(ctx: &egui::Context, name: &str, rgba: &[u8], size: usize) -> TextureHandle {
    let image = ColorImage::from_rgba_premultiplied([size, size], rgba);
    ctx.load_texture(name, image, options())
}

impl Textures {
    pub fn sync_browsers(&mut self, ctx: &egui::Context, browsers: &[Browser], rev: u64) {
        if self.browsers_rev == rev {
            return;
        }
        self.browsers_rev = rev;
        self.browser_icons = browsers
            .iter()
            .filter_map(|b| {
                let rgba = b.icon.as_ref()?;
                Some((
                    b.bundle_id.clone(),
                    texture(ctx, &b.bundle_id, rgba, ICON_SIZE),
                ))
            })
            .collect();
    }

    pub fn browser_icon(&mut self, ctx: &egui::Context, bundle_id: &str) -> Option<TextureHandle> {
        match self.browser_icons.get(bundle_id) {
            Some(icon) => Some(icon.clone()),
            None => self.app_icon(ctx, bundle_id),
        }
    }

    pub fn app_icon(&mut self, ctx: &egui::Context, bundle_id: &str) -> Option<TextureHandle> {
        self.app_icons
            .entry(bundle_id.to_owned())
            .or_insert_with(|| {
                let info = macos::app_info(bundle_id)?;
                let rgba = macos::app_icon(&info.path, SMALL_ICON_SIZE)?;
                Some(texture(ctx, bundle_id, &rgba, SMALL_ICON_SIZE))
            })
            .clone()
    }

    pub fn app_name(&mut self, bundle_id: &str) -> String {
        self.app_names
            .entry(bundle_id.to_owned())
            .or_insert_with(|| {
                macos::app_info(bundle_id).map_or_else(|| bundle_id.to_owned(), |i| i.name)
            })
            .clone()
    }

    pub fn symbol(&mut self, ctx: &egui::Context, name: &'static str) -> Option<TextureHandle> {
        self.symbols
            .entry(name)
            .or_insert_with(|| {
                let rgba = macos::symbol(name, SYMBOL_SIZE)?;
                Some(texture(ctx, name, &rgba, SYMBOL_SIZE))
            })
            .clone()
    }
}
