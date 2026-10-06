use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub default: Action,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alternative: Option<Action>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub picker_items: Option<Vec<Target>>,
    #[serde(default)]
    pub rules: Vec<Rule>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rule {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hosts: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub urls: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub source_apps: Vec<String>,
    pub action: Action,
}

impl Rule {
    pub fn has_conditions(&self) -> bool {
        !(self.hosts.is_empty() && self.urls.is_empty() && self.source_apps.is_empty())
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Action {
    #[default]
    Picker,
    Browser(Target),
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Target {
    pub bundle_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<String>,
}

const TEMPLATE: &str = r#"# Lightwell の設定ファイル
# ルールは上から順に評価され、最初にマッチしたものが使われます。
# 1つのルール内の条件 (hosts / urls / source_apps) はすべて満たす必要があり、省略した条件は「何でもよい」扱いです。
# 条件が1つもないルールは何にもマッチしません。

# どのルールにもマッチしなかったときの動作
[default]
type = "picker"
# type = "browser"
# bundle_id = "com.google.Chrome"
# profile = "Default"

# Fn キーを押しながら開いたときの動作 (ルールより優先)
# [alternative]
# type = "browser"
# bundle_id = "com.apple.Safari"

# 選択UIに並べるブラウザと順番。省略すると検出したものをすべて表示
# [[picker_items]]
# bundle_id = "com.google.Chrome"
# profile = "Profile 1"
#
# [[picker_items]]
# bundle_id = "com.apple.Safari"

# [[rules]]
# name = "仕事用"
# hosts = ["*.slack.com", "github.com"]
# source_apps = ["com.tinyspeck.slackmacgap"]
# [rules.action]
# type = "browser"
# bundle_id = "com.google.Chrome"
# profile = "Profile 1"
"#;

pub struct ConfigStore {
    path: PathBuf,
    config: Config,
    modified: Option<SystemTime>,
    error: Option<String>,
    created: bool,
}

impl ConfigStore {
    pub fn load() -> Self {
        let path = default_path();
        let created = !path.exists();
        if created {
            if let Some(dir) = path.parent() {
                let _ = fs::create_dir_all(dir);
            }
            let _ = fs::write(&path, TEMPLATE);
        }
        let mut store = Self {
            path,
            config: Config::default(),
            modified: None,
            error: None,
            created,
        };
        store.reload();
        store
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn config(&self) -> &Config {
        &self.config
    }

    pub fn created(&self) -> bool {
        self.created
    }

    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    /// 外部エディタでの編集を拾うため、URLを受け取るたびに呼ぶ。statだけなので十分安い
    pub fn reload_if_changed(&mut self) {
        if modified_time(&self.path) != self.modified {
            self.reload();
        }
    }

    pub fn reload(&mut self) {
        self.modified = modified_time(&self.path);
        match fs::read_to_string(&self.path) {
            Ok(text) => match toml::from_str(&text) {
                Ok(config) => {
                    self.config = config;
                    self.error = None;
                }
                // 壊れた設定で全リンクが開けなくなるのを避けるため、直前の設定を使い続ける
                Err(e) => self.error = Some(e.to_string()),
            },
            Err(e) => self.error = Some(e.to_string()),
        }
    }

    pub fn save(&mut self, config: Config) -> Result<(), String> {
        let text = toml::to_string_pretty(&config).map_err(|e| e.to_string())?;
        let tmp = self.path.with_extension("toml.tmp");
        fs::write(&tmp, text).map_err(|e| e.to_string())?;
        fs::rename(&tmp, &self.path).map_err(|e| e.to_string())?;
        self.config = config;
        self.error = None;
        self.modified = modified_time(&self.path);
        Ok(())
    }
}

fn default_path() -> PathBuf {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".config")
        });
    base.join("lightwell").join("config.toml")
}

fn modified_time(path: &Path) -> Option<SystemTime> {
    fs::metadata(path).and_then(|m| m.modified()).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn template_parses() {
        let config: Config = toml::from_str(TEMPLATE).unwrap();
        assert_eq!(config, Config::default());
    }

    #[test]
    fn roundtrip() {
        let config = Config {
            default: Action::Browser(Target {
                bundle_id: "com.google.Chrome".into(),
                profile: Some("Profile 1".into()),
            }),
            alternative: Some(Action::Browser(Target {
                bundle_id: "com.apple.Safari".into(),
                profile: None,
            })),
            picker_items: Some(vec![
                Target {
                    bundle_id: "com.google.Chrome".into(),
                    profile: Some("Default".into()),
                },
                Target {
                    bundle_id: "com.apple.Safari".into(),
                    profile: None,
                },
            ]),
            rules: vec![Rule {
                name: "work".into(),
                hosts: vec!["*.example.com".into()],
                source_apps: vec!["com.tinyspeck.slackmacgap".into()],
                action: Action::Picker,
                ..Default::default()
            }],
        };
        let text = toml::to_string_pretty(&config).unwrap();
        assert_eq!(toml::from_str::<Config>(&text).unwrap(), config);
    }

    #[test]
    fn empty_picker_items_is_distinct_from_unset() {
        let config = Config {
            picker_items: Some(Vec::new()),
            ..Default::default()
        };
        let text = toml::to_string_pretty(&config).unwrap();
        assert_eq!(toml::from_str::<Config>(&text).unwrap(), config);
    }
}
