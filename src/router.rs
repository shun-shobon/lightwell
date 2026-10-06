use crate::config::{Action, Config, Rule};

#[derive(Debug, Clone, Default)]
pub struct Request {
    pub url: String,
    pub source_app: Option<String>,
    pub fn_pressed: bool,
}

pub fn route<'a>(config: &'a Config, req: &Request) -> &'a Action {
    if req.fn_pressed
        && let Some(alternative) = &config.alternative
    {
        return alternative;
    }
    config
        .rules
        .iter()
        .find(|rule| matches(rule, req))
        .map(|rule| &rule.action)
        .unwrap_or(&config.default)
}

pub fn matches(rule: &Rule, req: &Request) -> bool {
    let host = host_of(&req.url);
    rule.has_conditions()
        && (rule.hosts.is_empty() || host.is_some_and(|h| rule.hosts.iter().any(|p| glob(p, h))))
        && (rule.urls.is_empty() || rule.urls.iter().any(|p| glob(p, &req.url)))
        && (rule.source_apps.is_empty()
            || req
                .source_app
                .as_deref()
                .is_some_and(|s| rule.source_apps.iter().any(|a| a.eq_ignore_ascii_case(s))))
}

pub fn host_of(url: &str) -> Option<&str> {
    let rest = &url[url.find("://")? + 3..];
    let authority = &rest[..rest.find(['/', '?', '#']).unwrap_or(rest.len())];
    let host_port = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
    let host = if host_port.starts_with('[') {
        &host_port[..host_port.find(']').map_or(host_port.len(), |i| i + 1)]
    } else {
        host_port.split(':').next().unwrap_or(host_port)
    };
    (!host.is_empty()).then_some(host)
}

/// `*` だけを特別扱いする、大文字小文字を区別しないglob
pub fn glob(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.to_lowercase().chars().collect();
    let t: Vec<char> = text.to_lowercase().chars().collect();
    let (mut pi, mut ti) = (0, 0);
    let mut backtrack: Option<(usize, usize)> = None;
    while ti < t.len() {
        if pi < p.len() && p[pi] == '*' {
            backtrack = Some((pi, ti));
            pi += 1;
        } else if pi < p.len() && p[pi] == t[ti] {
            pi += 1;
            ti += 1;
        } else if let Some((bp, bt)) = backtrack {
            pi = bp + 1;
            ti = bt + 1;
            backtrack = Some((bp, bt + 1));
        } else {
            return false;
        }
    }
    p[pi..].iter().all(|&c| c == '*')
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Target;

    #[test]
    fn glob_works() {
        assert!(glob("*.example.com", "a.b.example.com"));
        assert!(!glob("*.example.com", "example.com"));
        assert!(glob("example.com", "EXAMPLE.com"));
        assert!(glob(
            "https://github.com/*/pulls*",
            "https://github.com/foo/pulls?q=1"
        ));
        assert!(!glob("a*b", "acbd"));
        assert!(glob("*", ""));
    }

    #[test]
    fn host_extraction() {
        assert_eq!(
            host_of("https://user:pw@example.com:8080/a?b#c"),
            Some("example.com")
        );
        assert_eq!(host_of("http://[::1]:3000/"), Some("[::1]"));
        assert_eq!(host_of("https://example.com"), Some("example.com"));
        assert_eq!(host_of("file:///tmp/a.html"), None);
        assert_eq!(host_of("mailto:a@b"), None);
    }

    fn chrome() -> Action {
        Action::Browser(Target {
            bundle_id: "com.google.Chrome".into(),
            profile: None,
        })
    }

    #[test]
    fn first_matching_rule_wins() {
        let config = Config {
            rules: vec![
                Rule {
                    hosts: vec!["*.slack.com".into()],
                    source_apps: vec!["com.tinyspeck.slackmacgap".into()],
                    action: chrome(),
                    ..Default::default()
                },
                Rule {
                    hosts: vec!["*.slack.com".into()],
                    action: Action::Picker,
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        let mut req = Request {
            url: "https://foo.slack.com/x".into(),
            source_app: Some("com.tinyspeck.slackmacgap".into()),
            fn_pressed: false,
        };
        assert_eq!(route(&config, &req), &chrome());

        req.source_app = None;
        assert_eq!(route(&config, &req), &Action::Picker);
    }

    #[test]
    fn rule_without_conditions_never_matches() {
        let config = Config {
            default: chrome(),
            rules: vec![Rule::default()],
            ..Default::default()
        };
        let req = Request {
            url: "https://example.com".into(),
            ..Default::default()
        };
        assert_eq!(route(&config, &req), &chrome());
    }

    #[test]
    fn fn_key_uses_alternative_over_rules() {
        let safari = Action::Browser(Target {
            bundle_id: "com.apple.Safari".into(),
            profile: None,
        });
        let mut config = Config {
            rules: vec![Rule {
                hosts: vec!["example.com".into()],
                action: chrome(),
                ..Default::default()
            }],
            ..Default::default()
        };
        let mut req = Request {
            url: "https://example.com".into(),
            fn_pressed: true,
            ..Default::default()
        };
        assert_eq!(
            route(&config, &req),
            &chrome(),
            "alternative未設定なら通常通り"
        );

        config.alternative = Some(safari.clone());
        assert_eq!(route(&config, &req), &safari);

        req.fn_pressed = false;
        assert_eq!(route(&config, &req), &chrome());
    }
}
