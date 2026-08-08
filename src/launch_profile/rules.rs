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
    // quick-play feature flags (1.20+). normal launches leave these unset;
    // world quick launch enables only the singleplayer flag. listing every
    // flag explicitly keeps unrelated conditional arguments filtered out.
    pub has_quick_plays_support: Option<bool>,
    pub is_quick_play_singleplayer: Option<bool>,
    pub is_quick_play_multiplayer: Option<bool>,
    pub is_quick_play_realms: Option<bool>,
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
    let pairs = [
        (required.is_demo_user, current.is_demo_user),
        (
            required.has_custom_resolution,
            current.has_custom_resolution,
        ),
        (
            required.has_quick_plays_support,
            current.has_quick_plays_support,
        ),
        (
            required.is_quick_play_singleplayer,
            current.is_quick_play_singleplayer,
        ),
        (
            required.is_quick_play_multiplayer,
            current.is_quick_play_multiplayer,
        ),
        (required.is_quick_play_realms, current.is_quick_play_realms),
    ];
    pairs.iter().all(|(req, cur)| match req {
        Some(want) => cur.unwrap_or(false) == *want,
        None => true,
    })
}

#[cfg(test)]
#[path = "tests/rules.rs"]
mod tests;
