use std::path::{Path, PathBuf};

use objc2::rc::autoreleasepool;
use objc2_app_kit::NSWorkspace;
use objc2_foundation::{NSBundle, NSString, NSURL};

use crate::config::{Action, Target};
use crate::macos;

/// Retinaで48pt表示しても粗くならないサイズ
pub const ICON_SIZE: usize = 128;

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
    pub browser_name: String,
    pub profile_name: Option<String>,
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
                    icon: macos::app_icon(&path, ICON_SIZE),
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

/// 検出したすべてのブラウザ・プロファイル。
/// プロファイルを持つブラウザは「最後に使ったプロファイルで開く」項目も先頭に含める
pub fn all_choices(browsers: &[Browser]) -> Vec<Choice> {
    let mut out = Vec::new();
    for b in browsers {
        out.push(choice(
            browsers,
            Target {
                bundle_id: b.bundle_id.clone(),
                profile: None,
            },
        ));
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

/// 選択UIに実際に並べる項目
pub fn picker_choices(browsers: &[Browser], items: Option<&[Target]>) -> Vec<Choice> {
    let Some(items) = items else {
        return all_choices(browsers);
    };
    items
        .iter()
        .filter(|t| browsers.iter().any(|b| b.bundle_id == t.bundle_id))
        .map(|t| choice(browsers, t.clone()))
        .collect()
}

/// 設定画面に並べる (表示するもの, 表示しないもの)。
/// 表示しないものは all の順 (= ブラウザ名順) に並ぶ
pub fn picker_partition(all: &[Choice], items: Option<&[Target]>) -> (Vec<Target>, Vec<Target>) {
    let shown: Vec<Target> = match items {
        Some(items) => items.to_vec(),
        None => all.iter().map(|c| c.target.clone()).collect(),
    };
    let hidden = all
        .iter()
        .map(|c| c.target.clone())
        .filter(|t| !shown.contains(t))
        .collect();
    (shown, hidden)
}

fn choice(browsers: &[Browser], target: Target) -> Choice {
    let browser = browsers.iter().find(|b| b.bundle_id == target.bundle_id);
    let profile_name = target.profile.as_ref().map(|dir| {
        browser
            .and_then(|b| b.profiles.iter().find(|p| &p.dir == dir))
            .map_or(dir.clone(), |p| p.name.clone())
    });
    Choice {
        bundle_id: target.bundle_id.clone(),
        label: describe_target(browsers, &target),
        browser_name: browser.map_or(target.bundle_id.clone(), |b| b.name.clone()),
        profile_name,
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

pub fn chromium_user_data_dir(bundle_id: &str) -> Option<PathBuf> {
    let dir = match bundle_id {
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
    };
    let home = PathBuf::from(std::env::var_os("HOME").unwrap_or_default());
    Some(home.join("Library/Application Support").join(dir))
}

fn chromium_profiles(bundle_id: &str) -> Vec<Profile> {
    let Some(dir) = chromium_user_data_dir(bundle_id) else {
        return Vec::new();
    };
    std::fs::read(dir.join("Local State"))
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

#[cfg(test)]
mod tests {
    use super::*;

    fn browser(bundle_id: &str, profiles: &[&str]) -> Browser {
        Browser {
            bundle_id: bundle_id.into(),
            name: bundle_id.into(),
            app_path: PathBuf::new(),
            profiles: profiles
                .iter()
                .map(|d| Profile {
                    dir: (*d).into(),
                    name: (*d).into(),
                })
                .collect(),
            icon: None,
        }
    }

    fn target(bundle_id: &str, profile: Option<&str>) -> Target {
        Target {
            bundle_id: bundle_id.into(),
            profile: profile.map(Into::into),
        }
    }

    #[test]
    fn profiled_browser_is_listed_with_and_without_profile() {
        let browsers = [
            browser("chrome", &["Default", "Profile 1"]),
            browser("safari", &[]),
        ];
        let targets: Vec<_> = all_choices(&browsers)
            .into_iter()
            .map(|c| (c.target.bundle_id, c.target.profile))
            .collect();
        assert_eq!(
            targets,
            [
                ("chrome".into(), None),
                ("chrome".into(), Some("Default".into())),
                ("chrome".into(), Some("Profile 1".into())),
                ("safari".into(), None),
            ]
        );
    }

    #[test]
    fn picker_shows_listed_installed_items_in_order() {
        let browsers = [browser("chrome", &["Default"]), browser("safari", &[])];
        let items = [
            target("safari", None),
            target("removed", None),
            target("chrome", Some("Default")),
        ];
        let shown: Vec<_> = picker_choices(&browsers, Some(&items))
            .into_iter()
            .map(|c| c.target)
            .collect();
        assert_eq!(
            shown,
            [target("safari", None), target("chrome", Some("Default"))]
        );
        assert_eq!(picker_choices(&browsers, None).len(), 3);
        assert!(picker_choices(&browsers, Some(&[])).is_empty());
    }

    #[test]
    fn picker_partition_keeps_hidden_in_name_order() {
        let browsers = [browser("chrome", &["Default"]), browser("safari", &[])];
        let all = all_choices(&browsers);
        let (shown, hidden) = picker_partition(&all, Some(&[target("safari", None)]));
        assert_eq!(shown, [target("safari", None)]);
        assert_eq!(
            hidden,
            [target("chrome", None), target("chrome", Some("Default"))]
        );
        let (shown, hidden) = picker_partition(&all, None);
        assert_eq!(shown.len(), 3);
        assert!(hidden.is_empty());
    }

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
