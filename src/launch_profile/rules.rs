// mojang rule evaluation. profiles, libraries, and argument entries can
// carry conditional rules that filter them by OS, architecture, or feature
// flags. this module is the single source of truth for that semantics -
// see `evaluate` below for the exact rules.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum RuleAction {
    Allow,
    Disallow,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize, Serialize)]
pub struct OsCondition {
    pub name: Option<String>,
    pub arch: Option<String>,
    // mojang occasionally constrains natives selection on os.version with a
    // regex. rare in practice - when present, it's a substring/anchor match
    // against the host OS version reported by `system::mojang_os_version`.
    pub version: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct FeatureSet {
    pub is_demo_user: Option<bool>,
    pub has_custom_resolution: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct Rule {
    pub action: RuleAction,
    pub os: Option<OsCondition>,
    pub features: Option<FeatureSet>,
}

pub struct RuleContext<'a> {
    pub os_name: &'a str,
    pub os_version: &'a str,
    pub arch: &'a str,
    pub features: &'a FeatureSet,
}

pub fn evaluate(rules: &[Rule], ctx: &RuleContext) -> bool {
    if rules.is_empty() {
        return true;
    }
    let mut allowed = false;
    for rule in rules {
        if rule_matches(rule, ctx) {
            allowed = matches!(rule.action, RuleAction::Allow);
        }
    }
    allowed
}

fn rule_matches(rule: &Rule, ctx: &RuleContext) -> bool {
    if let Some(os) = &rule.os {
        if let Some(name) = &os.name
            && name != ctx.os_name
        {
            return false;
        }
        if let Some(arch) = &os.arch
            && arch != ctx.arch
        {
            return false;
        }
        if let Some(pattern) = &os.version
            && !os_version_matches(pattern, ctx.os_version)
        {
            return false;
        }
    }
    if let Some(required) = &rule.features
        && !features_match(required, ctx.features)
    {
        return false;
    }
    true
}

// mojang's os.version constraints are typically anchored regex patterns
// (e.g. `^10\\.`). we do a substring containment check as a defensive
// approximation that doesn't pull in the `regex` crate. when the host
// os_version is empty (Windows fallback path returns ""), version-gated
// rules don't match - which is the conservative default.
fn os_version_matches(pattern: &str, host_version: &str) -> bool {
    if host_version.is_empty() {
        return false;
    }
    // strip common regex anchors and metacharacters for substring lookup.
    // good enough for the rare profile that uses os.version.
    let needle = pattern
        .trim_start_matches('^')
        .trim_end_matches('$')
        .trim_end_matches('.')
        .trim_end_matches('\\');
    host_version.contains(needle)
}

fn features_match(required: &FeatureSet, current: &FeatureSet) -> bool {
    if let Some(want) = required.is_demo_user
        && current.is_demo_user.unwrap_or(false) != want
    {
        return false;
    }
    if let Some(want) = required.has_custom_resolution
        && current.has_custom_resolution.unwrap_or(false) != want
    {
        return false;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn linux_ctx<'a>(features: &'a FeatureSet) -> RuleContext<'a> {
        RuleContext {
            os_name: "linux",
            os_version: "6.0",
            arch: "x86_64",
            features,
        }
    }

    #[test]
    fn empty_rules_allow() {
        let features = FeatureSet::default();
        let ctx = linux_ctx(&features);
        assert!(evaluate(&[], &ctx));
    }

    #[test]
    fn single_allow_matching_os_allows() {
        let rules = vec![Rule {
            action: RuleAction::Allow,
            os: Some(OsCondition {
                name: Some("linux".into()),
                arch: None,
                version: None,
            }),
            features: None,
        }];
        let features = FeatureSet::default();
        let ctx = linux_ctx(&features);
        assert!(evaluate(&rules, &ctx));
    }

    #[test]
    fn single_disallow_matching_os_disallows() {
        let rules = vec![Rule {
            action: RuleAction::Disallow,
            os: Some(OsCondition {
                name: Some("linux".into()),
                arch: None,
                version: None,
            }),
            features: None,
        }];
        let features = FeatureSet::default();
        let ctx = linux_ctx(&features);
        assert!(!evaluate(&rules, &ctx));
    }

    #[test]
    fn allow_without_os_match_disallows_by_default() {
        // explicit allow for windows; we are on linux; nothing matches;
        // default state remains disallow.
        let rules = vec![Rule {
            action: RuleAction::Allow,
            os: Some(OsCondition {
                name: Some("windows".into()),
                arch: None,
                version: None,
            }),
            features: None,
        }];
        let features = FeatureSet::default();
        let ctx = linux_ctx(&features);
        assert!(!evaluate(&rules, &ctx));
    }

    #[test]
    fn last_matching_rule_wins_when_allow_then_disallow() {
        let rules = vec![
            Rule {
                action: RuleAction::Allow,
                os: None,
                features: None,
            },
            Rule {
                action: RuleAction::Disallow,
                os: Some(OsCondition {
                    name: Some("osx".into()),
                    arch: None,
                    version: None,
                }),
                features: None,
            },
        ];
        let features = FeatureSet::default();
        let osx_ctx = RuleContext {
            os_name: "osx",
            os_version: "6.0",
            arch: "x86_64",
            features: &features,
        };
        assert!(!evaluate(&rules, &osx_ctx));
        // and on linux: only the first rule matches → still allow.
        let lin = linux_ctx(&features);
        assert!(evaluate(&rules, &lin));
    }

    #[test]
    fn last_matching_rule_wins_when_disallow_then_allow() {
        let rules = vec![
            Rule {
                action: RuleAction::Disallow,
                os: None,
                features: None,
            },
            Rule {
                action: RuleAction::Allow,
                os: Some(OsCondition {
                    name: Some("linux".into()),
                    arch: None,
                    version: None,
                }),
                features: None,
            },
        ];
        let features = FeatureSet::default();
        let ctx = linux_ctx(&features);
        assert!(evaluate(&rules, &ctx));
    }

    #[test]
    fn arch_mismatch_blocks_rule() {
        let rules = vec![Rule {
            action: RuleAction::Allow,
            os: Some(OsCondition {
                name: Some("linux".into()),
                arch: Some("arm64".into()),
                version: None,
            }),
            features: None,
        }];
        let features = FeatureSet::default();
        let ctx = linux_ctx(&features);
        // we are on linux but x86_64, not arm64 → rule does not match.
        assert!(!evaluate(&rules, &ctx));
    }

    #[test]
    fn feature_required_true_matches_when_ctx_true() {
        let rules = vec![Rule {
            action: RuleAction::Allow,
            os: None,
            features: Some(FeatureSet {
                is_demo_user: Some(true),
                has_custom_resolution: None,
            }),
        }];
        let demo_features = FeatureSet {
            is_demo_user: Some(true),
            has_custom_resolution: None,
        };
        let ctx = RuleContext {
            os_name: "linux",
            os_version: "6.0",
            arch: "x86_64",
            features: &demo_features,
        };
        assert!(evaluate(&rules, &ctx));
    }

    #[test]
    fn feature_required_true_blocks_when_ctx_false() {
        let rules = vec![Rule {
            action: RuleAction::Allow,
            os: None,
            features: Some(FeatureSet {
                is_demo_user: Some(true),
                has_custom_resolution: None,
            }),
        }];
        let features = FeatureSet::default();
        let ctx = linux_ctx(&features);
        assert!(!evaluate(&rules, &ctx));
    }

    #[test]
    fn no_os_no_features_rule_always_matches() {
        let rules = vec![Rule {
            action: RuleAction::Allow,
            os: None,
            features: None,
        }];
        let features = FeatureSet::default();
        let ctx = linux_ctx(&features);
        assert!(evaluate(&rules, &ctx));
    }

    #[test]
    fn os_version_pattern_matches_against_host() {
        let rules = vec![Rule {
            action: RuleAction::Allow,
            os: Some(OsCondition {
                name: Some("osx".into()),
                arch: None,
                version: Some("^10\\.".into()),
            }),
            features: None,
        }];
        let features = FeatureSet::default();
        let ctx_match = RuleContext {
            os_name: "osx",
            os_version: "10.15.7",
            arch: "x86_64",
            features: &features,
        };
        assert!(evaluate(&rules, &ctx_match));
        let ctx_mismatch = RuleContext {
            os_name: "osx",
            os_version: "13.2.1",
            arch: "x86_64",
            features: &features,
        };
        assert!(!evaluate(&rules, &ctx_mismatch));
    }

    #[test]
    fn os_version_pattern_does_not_match_when_host_unknown() {
        let rules = vec![Rule {
            action: RuleAction::Allow,
            os: Some(OsCondition {
                name: Some("windows".into()),
                arch: None,
                version: Some("^10\\.".into()),
            }),
            features: None,
        }];
        let features = FeatureSet::default();
        let ctx = RuleContext {
            os_name: "windows",
            os_version: "",
            arch: "x86_64",
            features: &features,
        };
        assert!(!evaluate(&rules, &ctx));
    }

    #[test]
    fn rule_deserializes_from_mojang_json() {
        // shape lifted from real mojang library rules.
        let json = r#"{
            "action": "allow",
            "os": { "name": "osx" }
        }"#;
        let rule: Rule = serde_json::from_str(json).unwrap();
        assert_eq!(rule.action, RuleAction::Allow);
        assert_eq!(
            rule.os.as_ref().and_then(|o| o.name.as_deref()),
            Some("osx")
        );
        assert!(rule.os.as_ref().and_then(|o| o.arch.as_ref()).is_none());
        assert!(rule.features.is_none());
    }

    #[test]
    fn rule_deserializes_with_features() {
        let json = r#"{
            "action": "allow",
            "features": { "is_demo_user": true }
        }"#;
        let rule: Rule = serde_json::from_str(json).unwrap();
        assert_eq!(rule.action, RuleAction::Allow);
        assert!(rule.os.is_none());
        assert_eq!(
            rule.features.as_ref().and_then(|f| f.is_demo_user),
            Some(true)
        );
    }
}
