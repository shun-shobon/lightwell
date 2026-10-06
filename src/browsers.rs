use std::path::{Path, PathBuf};

use objc2::AnyThread;
use objc2::rc::autoreleasepool;
use objc2_app_kit::{
    NSBitmapImageRep, NSCompositingOperation, NSDeviceRGBColorSpace, NSGraphicsContext, NSWorkspace,
};
use objc2_foundation::{NSBundle, NSPoint, NSRect, NSSize, NSString, NSURL};

use crate::config::{Action, Target};

pub const ICON_SIZE: usize = 64;

pub struct Browser {
    pub bundle_id: String,
    pub name: String,
    pub app_path: PathBuf,
    pub profiles: Vec<Profile>,
    /// 乗算済みアルファのRGBA (ICON_SIZE x ICON_SIZE)
    pub icon: Option<Vec<u8>>,
}

pub struct Profile {
    pub dir: String,
    pub name: String,
}

/// 選択UIやブラウザ選択コンボに並べる1項目
#[derive(Clone)]
pub struct Choice {
    pub bundle_id: String,
    pub label: String,
    pub target: Target,
}

pub fn discover(own_bundle_id: &str) -> Vec<Browser> {
    autoreleasepool(|_| {
        let workspace = NSWorkspace::sharedWorkspace();
        let probe = NSURL::URLWithString(&NSString::from_str("https://example.com")).unwrap();
        let mut browsers: Vec<Browser> = workspace
            .URLsForApplicationsToOpenURL(&probe)
            .iter()
            .filter_map(|url| {
                let bundle = NSBundle::bundleWithURL(&url)?;
                let bundle_id = bundle.bundleIdentifier()?.to_string();
                if bundle_id == own_bundle_id {
                    return None;
                }
                let path = PathBuf::from(url.path()?.to_string());
                let name = app_display_name(&path);
                Some(Browser {
                    profiles: chromium_profiles(&bundle_id),
                    icon: render_icon(&workspace, &path),
                    bundle_id,
                    name,
                    app_path: path,
                })
            })
            .collect();
        // 同じアプリが複数の場所にあると重複して返ってくる
        browsers.sort_by_key(|a| a.name.to_lowercase());
        browsers.dedup_by(|a, b| a.bundle_id == b.bundle_id);
        browsers
    })
}

/// 検出したすべてのブラウザ・プロファイル
pub fn all_choices(browsers: &[Browser]) -> Vec<Choice> {
    let mut out = Vec::new();
    for b in browsers {
        if b.profiles.is_empty() {
            out.push(choice(
                browsers,
                Target {
                    bundle_id: b.bundle_id.clone(),
                    profile: None,
                },
            ));
        }
        for p in &b.profiles {
            out.push(choice(
                browsers,
                Target {
                    bundle_id: b.bundle_id.clone(),
                    profile: Some(p.dir.clone()),
                },
            ));
        }
    }
    out
}

/// 選択UIに実際に並べる項目。設定がなければ検出したものすべて
pub fn picker_choices(browsers: &[Browser], items: &[Target]) -> Vec<Choice> {
    if items.is_empty() {
        return all_choices(browsers);
    }
    items
        .iter()
        .filter(|t| browsers.iter().any(|b| b.bundle_id == t.bundle_id))
        .map(|t| choice(browsers, t.clone()))
        .collect()
}

fn choice(browsers: &[Browser], target: Target) -> Choice {
    Choice {
        bundle_id: target.bundle_id.clone(),
        label: describe_target(browsers, &target),
        target,
    }
}

pub fn describe(browsers: &[Browser], action: &Action) -> String {
    match action {
        Action::Picker => "選択UIを表示".to_owned(),
        Action::Browser(target) => describe_target(browsers, target),
    }
}

pub fn describe_target(browsers: &[Browser], target: &Target) -> String {
    let browser = browsers.iter().find(|b| b.bundle_id == target.bundle_id);
    let name = browser.map_or(target.bundle_id.as_str(), |b| b.name.as_str());
    match &target.profile {
        None => name.to_owned(),
        Some(dir) => {
            let profile_name = browser
                .and_then(|b| b.profiles.iter().find(|p| &p.dir == dir))
                .map_or(dir.as_str(), |p| p.name.as_str());
            format!("{name} — {profile_name}")
        }
    }
}

fn app_display_name(path: &Path) -> String {
    path.file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn chromium_user_data_dir(bundle_id: &str) -> Option<&'static str> {
    Some(match bundle_id {
        "com.google.Chrome" => "Google/Chrome",
        "com.google.Chrome.beta" => "Google/Chrome Beta",
        "com.google.Chrome.dev" => "Google/Chrome Dev",
        "com.google.Chrome.canary" => "Google/Chrome Canary",
        "org.chromium.Chromium" => "Chromium",
        "com.brave.Browser" => "BraveSoftware/Brave-Browser",
        "com.brave.Browser.beta" => "BraveSoftware/Brave-Browser-Beta",
        "com.brave.Browser.nightly" => "BraveSoftware/Brave-Browser-Nightly",
        "com.microsoft.edgemac" => "Microsoft Edge",
        "com.microsoft.edgemac.Beta" => "Microsoft Edge Beta",
        "com.microsoft.edgemac.Dev" => "Microsoft Edge Dev",
        "com.vivaldi.Vivaldi" => "Vivaldi",
        _ => return None,
    })
}

fn chromium_profiles(bundle_id: &str) -> Vec<Profile> {
    let Some(dir) = chromium_user_data_dir(bundle_id) else {
        return Vec::new();
    };
    let home = PathBuf::from(std::env::var_os("HOME").unwrap_or_default());
    let local_state = home
        .join("Library/Application Support")
        .join(dir)
        .join("Local State");
    std::fs::read(local_state)
        .ok()
        .map(|bytes| parse_local_state(&bytes))
        .unwrap_or_default()
}

fn parse_local_state(bytes: &[u8]) -> Vec<Profile> {
    let Ok(json) = serde_json::from_slice::<serde_json::Value>(bytes) else {
        return Vec::new();
    };
    let Some(cache) = json
        .pointer("/profile/info_cache")
        .and_then(|v| v.as_object())
    else {
        return Vec::new();
    };
    let order: Vec<&str> = json
        .pointer("/profile/profiles_order")
        .and_then(|v| v.as_array())
        .map(|a| a.iter().filter_map(|v| v.as_str()).collect())
        .unwrap_or_default();
    let mut profiles: Vec<Profile> = cache
        .iter()
        .map(|(dir, info)| Profile {
            dir: dir.clone(),
            name: info
                .get("name")
                .and_then(|n| n.as_str())
                .unwrap_or(dir)
                .to_owned(),
        })
        .collect();
    profiles.sort_by_key(|p| {
        (
            order.iter().position(|d| *d == p.dir).unwrap_or(usize::MAX),
            p.dir != "Default",
            p.dir.clone(),
        )
    });
    profiles
}

fn render_icon(workspace: &NSWorkspace, path: &Path) -> Option<Vec<u8>> {
    let image = workspace.iconForFile(&NSString::from_str(&path.to_string_lossy()));
    let size = ICON_SIZE as isize;
    let rep = unsafe {
        NSBitmapImageRep::initWithBitmapDataPlanes_pixelsWide_pixelsHigh_bitsPerSample_samplesPerPixel_hasAlpha_isPlanar_colorSpaceName_bytesPerRow_bitsPerPixel(
            NSBitmapImageRep::alloc(),
            std::ptr::null_mut(),
            size,
            size,
            8,
            4,
            true,
            false,
            NSDeviceRGBColorSpace,
            size * 4,
            32,
        )
    }?;
    let ctx = NSGraphicsContext::graphicsContextWithBitmapImageRep(&rep)?;
    NSGraphicsContext::saveGraphicsState_class();
    NSGraphicsContext::setCurrentContext(Some(&ctx));
    let rect = NSRect::new(
        NSPoint::new(0.0, 0.0),
        NSSize::new(size as f64, size as f64),
    );
    image.drawInRect_fromRect_operation_fraction(
        rect,
        NSRect::ZERO,
        NSCompositingOperation::Copy,
        1.0,
    );
    ctx.flushGraphics();
    NSGraphicsContext::restoreGraphicsState_class();

    let data = rep.bitmapData();
    if data.is_null() {
        return None;
    }
    let len = (rep.bytesPerRow() * size) as usize;
    Some(unsafe { std::slice::from_raw_parts(data, len) }.to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_state_profiles_are_ordered() {
        let json = br#"{"profile":{"info_cache":{
            "Profile 2":{"name":"Work"},
            "Default":{"name":"Personal"},
            "Profile 1":{"name":"Test"}
        }}}"#;
        let profiles = parse_local_state(json);
        let dirs: Vec<_> = profiles.iter().map(|p| p.dir.as_str()).collect();
        assert_eq!(dirs, ["Default", "Profile 1", "Profile 2"]);
        assert_eq!(profiles[2].name, "Work");
    }
}
