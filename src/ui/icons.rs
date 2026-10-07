use std::cell::RefCell;
use std::collections::HashMap;

use base64::Engine;
use dioxus::prelude::*;

use crate::browsers::Browser;
use crate::macos;

const APP_ICON_SIZE: usize = 64;
const SYMBOL_SIZE: usize = 64;

pub fn data_url(png: &[u8]) -> String {
    let base64 = base64::engine::general_purpose::STANDARD.encode(png);
    format!("data:image/png;base64,{base64}")
}

thread_local! {
    // ラスタライズは数ミリ秒かかるので、描画のたびに作り直さない
    static CACHE: RefCell<HashMap<String, Option<String>>> = RefCell::default();
}

fn cached(key: String, make: impl FnOnce() -> Option<Vec<u8>>) -> Option<String> {
    CACHE.with_borrow_mut(|cache| {
        cache
            .entry(key)
            .or_insert_with(|| make().map(|png| data_url(&png)))
            .clone()
    })
}

pub fn app_icon(bundle_id: &str) -> Option<String> {
    cached(format!("app:{bundle_id}"), || {
        macos::app_icon(&macos::app_path_for_bundle_id(bundle_id)?, APP_ICON_SIZE)
    })
}

pub fn browser_icon(browsers: &[Browser], bundle_id: &str) -> Option<String> {
    match browsers.iter().find(|b| b.bundle_id == bundle_id) {
        Some(browser) => browser.icon.clone(),
        None => app_icon(bundle_id),
    }
}

#[component]
pub fn Icon(src: Option<String>, size: u32) -> Element {
    rsx! {
        if let Some(src) = src {
            img { class: "icon", src, width: size, height: size }
        } else {
            span { class: "icon", style: "width: {size}px; height: {size}px" }
        }
    }
}

/// SF Symbols を文字色で塗るため、画像ではなくマスクとして使う
#[component]
pub fn Symbol(name: &'static str, size: u32) -> Element {
    let url = cached(format!("symbol:{name}"), || {
        macos::symbol(name, SYMBOL_SIZE)
    });
    let mask = url.map_or_else(String::new, |url| {
        format!("-webkit-mask-image: url({url});")
    });
    rsx! {
        span { class: "symbol", style: "width: {size}px; height: {size}px; {mask}" }
    }
}
