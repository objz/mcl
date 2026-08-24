// SPDX-FileCopyrightText: 2026 Constantin Bauer
// SPDX-License-Identifier: GPL-3.0-only

use super::*;
use crate::launch_profile::model::{Argument, ArgumentValue, AssetIndex, JavaVersion, Library};
use crate::launch_profile::rules::{Rule, RuleAction};

fn empty_profile(id: &str) -> LaunchProfile {
    LaunchProfile {
        id: id.into(),
        ..Default::default()
    }
}

fn lib(name: &str) -> Library {
    Library {
        name: name.into(),
        ..Default::default()
    }
}

fn allow_linux_rule() -> Rule {
    Rule {
        action: RuleAction::Allow,
        os: Some(crate::launch_profile::rules::OsCondition {
            name: Some("linux".into()),
            ..Default::default()
        }),
        features: None,
    }
}

#[test]
fn child_id_wins() {
    let mut child = empty_profile("child");
    let parent = empty_profile("parent");
    child.main_class = None;
    let merged = merge_into(child, parent);
    assert_eq!(merged.id, "child");
}

#[test]
fn merge_carries_parent_inherits_from() {
    // merge_into preserves parent's inherits_from so resolve() can keep
    // walking. resolve() itself clears the final result's inherits_from
    // after the loop exits.
    let mut child = empty_profile("child");
    child.inherits_from = Some("parent".into());
    let mut parent = empty_profile("parent");
    parent.inherits_from = Some("grandparent".into());
    let merged = merge_into(child, parent);
    assert_eq!(merged.inherits_from.as_deref(), Some("grandparent"));
}

#[test]
fn merge_with_root_parent_clears_inherits_from() {
    // parent with no inherits_from means the chain ends.
    let mut child = empty_profile("child");
    child.inherits_from = Some("parent".into());
    let parent = empty_profile("parent");
    let merged = merge_into(child, parent);
    assert!(merged.inherits_from.is_none());
}

#[test]
fn child_main_class_overrides_parent() {
    let mut child = empty_profile("child");
    let mut parent = empty_profile("parent");
    child.main_class = Some("child.Main".into());
    parent.main_class = Some("parent.Main".into());
    let merged = merge_into(child, parent);
    assert_eq!(merged.main_class.as_deref(), Some("child.Main"));
}

#[test]
fn parent_main_class_used_when_child_missing() {
    let child = empty_profile("child");
    let mut parent = empty_profile("parent");
    parent.main_class = Some("parent.Main".into());
    let merged = merge_into(child, parent);
    assert_eq!(merged.main_class.as_deref(), Some("parent.Main"));
}

#[test]
fn libraries_are_concatenated_parent_first() {
    let mut child = empty_profile("child");
    let mut parent = empty_profile("parent");
    parent.libraries = vec![lib("p1"), lib("p2")];
    child.libraries = vec![lib("c1")];
    let merged = merge_into(child, parent);
    let names: Vec<_> = merged.libraries.iter().map(|l| l.name.as_str()).collect();
    assert_eq!(names, vec!["p1", "p2", "c1"]);
}

#[test]
fn child_library_supersedes_parent_with_same_group_artifact() {
    // forge declares log4j 2.17.0; vanilla declared 2.0-beta9. without
    // dedup, both end up on the classpath and the JVM picks the first
    // (vanilla) match - defeating forge's override. dedup keeps child's.
    let mut child = empty_profile("forge");
    let mut parent = empty_profile("vanilla");
    parent.libraries = vec![
        lib("org.apache.logging.log4j:log4j-core:2.0-beta9"),
        lib("org.lwjgl:lwjgl:3.3.1"),
    ];
    child.libraries = vec![
        lib("org.apache.logging.log4j:log4j-core:2.17.0"),
        lib("net.minecraftforge:forge:47.2.0"),
    ];
    let merged = merge_into(child, parent);
    let names: Vec<_> = merged.libraries.iter().map(|l| l.name.as_str()).collect();
    // parent's log4j-core is filtered (superseded by child); parent's
    // lwjgl stays (no conflict); child's log4j and forge come last.
    assert_eq!(
        names,
        vec![
            "org.lwjgl:lwjgl:3.3.1",
            "org.apache.logging.log4j:log4j-core:2.17.0",
            "net.minecraftforge:forge:47.2.0",
        ]
    );
}

#[test]
fn coord_key_extracts_group_artifact() {
    assert_eq!(coord_key("org.lwjgl:lwjgl:3.3.1"), "org.lwjgl:lwjgl");
    assert_eq!(
        coord_key("org.apache.logging.log4j:log4j-core:2.17.0"),
        "org.apache.logging.log4j:log4j-core"
    );
    // with classifier
    assert_eq!(
        coord_key("org.lwjgl:lwjgl:3.3.1:natives-linux"),
        "org.lwjgl:lwjgl"
    );
    // malformed (no colons) - return as-is
    assert_eq!(coord_key("malformed"), "malformed");
}

#[test]
fn arguments_are_concatenated_parent_first() {
    let mut child = empty_profile("child");
    let mut parent = empty_profile("parent");
    parent.arguments = Some(Arguments {
        game: vec![Argument::Literal("--from-parent-game".into())],
        jvm: vec![Argument::Literal("--from-parent-jvm".into())],
    });
    child.arguments = Some(Arguments {
        game: vec![Argument::Literal("--from-child-game".into())],
        jvm: vec![Argument::Literal("--from-child-jvm".into())],
    });
    let merged = merge_into(child, parent);
    let args = merged.arguments.expect("arguments present");
    assert_eq!(
        args.game,
        vec![
            Argument::Literal("--from-parent-game".into()),
            Argument::Literal("--from-child-game".into()),
        ]
    );
    assert_eq!(
        args.jvm,
        vec![
            Argument::Literal("--from-parent-jvm".into()),
            Argument::Literal("--from-child-jvm".into()),
        ]
    );
}

#[test]
fn arguments_from_child_only_carry_through() {
    let mut child = empty_profile("child");
    let parent = empty_profile("parent");
    child.arguments = Some(Arguments {
        game: vec![Argument::Literal("--child".into())],
        jvm: Vec::new(),
    });
    let merged = merge_into(child, parent);
    let args = merged.arguments.expect("arguments present");
    assert_eq!(args.game.len(), 1);
    assert!(args.jvm.is_empty());
}

#[test]
fn arguments_from_parent_only_carry_through() {
    let child = empty_profile("child");
    let mut parent = empty_profile("parent");
    parent.arguments = Some(Arguments {
        game: Vec::new(),
        jvm: vec![Argument::Literal("--parent-jvm".into())],
    });
    let merged = merge_into(child, parent);
    let args = merged.arguments.expect("arguments present");
    assert!(args.game.is_empty());
    assert_eq!(args.jvm.len(), 1);
}

#[test]
fn conditional_arguments_with_rules_survive_merge() {
    // make sure the Argument::Conditional shape isn't accidentally
    // flattened or filtered during merging - rule eval happens later
    // at render time, not during merge.
    let mut child = empty_profile("child");
    let parent = empty_profile("parent");
    child.arguments = Some(Arguments {
        game: vec![Argument::Conditional {
            rules: vec![allow_linux_rule()],
            value: ArgumentValue::Single("--linux-only".into()),
        }],
        jvm: Vec::new(),
    });
    let merged = merge_into(child, parent);
    let args = merged.arguments.expect("arguments present");
    match &args.game[0] {
        Argument::Conditional { rules, .. } => {
            assert_eq!(rules.len(), 1);
            assert_eq!(rules[0].action, RuleAction::Allow);
        }
        _ => panic!("expected conditional argument to survive merge"),
    }
}

#[test]
fn legacy_minecraft_arguments_child_overrides_parent() {
    let mut child = empty_profile("child");
    let mut parent = empty_profile("parent");
    child.minecraft_arguments = Some("--child".into());
    parent.minecraft_arguments = Some("--parent".into());
    let merged = merge_into(child, parent);
    assert_eq!(merged.minecraft_arguments.as_deref(), Some("--child"));
}

#[test]
fn asset_index_inherits_from_parent_when_child_absent() {
    let child = empty_profile("child");
    let mut parent = empty_profile("parent");
    parent.asset_index = Some(AssetIndex {
        id: "5".into(),
        url: "https://example.invalid/5.json".into(),
        sha1: "0".repeat(40),
        size: None,
        total_size: None,
    });
    let merged = merge_into(child, parent);
    assert!(merged.asset_index.is_some());
    assert_eq!(merged.asset_index.unwrap().id, "5");
}

#[test]
fn java_version_inherits_from_parent_when_child_absent() {
    let child = empty_profile("child");
    let mut parent = empty_profile("parent");
    parent.java_version = Some(JavaVersion {
        component: Some("java-runtime-gamma".into()),
        major_version: 17,
    });
    let merged = merge_into(child, parent);
    assert_eq!(
        merged.java_version.as_ref().map(|j| j.major_version),
        Some(17)
    );
}

use tempfile::TempDir;

fn write_profile(meta_dir: &Path, profile: &LaunchProfile) {
    let path = meta_dir
        .join("cache/minecraft/versions")
        .join(&profile.id)
        .join("meta.json");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let json = serde_json::to_string_pretty(profile).unwrap();
    std::fs::write(&path, json).unwrap();
}

#[tokio::test]
async fn resolve_returns_unchanged_when_no_inherits_from() {
    let tmp = TempDir::new().unwrap();
    let profile = empty_profile("standalone");
    let resolved = resolve(profile, tmp.path()).await.unwrap();
    assert_eq!(resolved.id, "standalone");
    assert!(resolved.inherits_from.is_none());
}

#[tokio::test]
async fn resolve_single_level_inheritance_merges_parent() {
    let tmp = TempDir::new().unwrap();

    let mut parent = empty_profile("1.20.1");
    parent.main_class = Some("net.minecraft.client.main.Main".into());
    parent.libraries = vec![lib("vanilla-lib")];
    write_profile(tmp.path(), &parent);

    let mut child = empty_profile("1.20.1-forge-47.2.0");
    child.inherits_from = Some("1.20.1".into());
    child.libraries = vec![lib("forge-lib")];

    let resolved = resolve(child, tmp.path()).await.unwrap();
    assert_eq!(resolved.id, "1.20.1-forge-47.2.0");
    assert!(resolved.inherits_from.is_none());
    assert_eq!(
        resolved.main_class.as_deref(),
        Some("net.minecraft.client.main.Main")
    );
    let names: Vec<_> = resolved.libraries.iter().map(|l| l.name.as_str()).collect();
    assert_eq!(names, vec!["vanilla-lib", "forge-lib"]);
}

#[tokio::test]
async fn resolve_errors_when_parent_missing() {
    let tmp = TempDir::new().unwrap();

    let mut child = empty_profile("1.20.1-forge-47.2.0");
    child.inherits_from = Some("1.20.1".into());

    let err = resolve(child, tmp.path()).await.unwrap_err();
    assert!(
        matches!(err, ResolveError::ParentNotFound(_)),
        "expected ParentNotFound, got {err:?}"
    );
}

#[tokio::test]
async fn resolve_errors_when_parent_is_invalid_json() {
    let tmp = TempDir::new().unwrap();

    let parent_path = tmp
        .path()
        .join("cache/minecraft/versions")
        .join("1.20.1")
        .join("meta.json");
    std::fs::create_dir_all(parent_path.parent().unwrap()).unwrap();
    std::fs::write(&parent_path, "{ not valid json").unwrap();

    let mut child = empty_profile("1.20.1-forge-47.2.0");
    child.inherits_from = Some("1.20.1".into());

    let err = resolve(child, tmp.path()).await.unwrap_err();
    assert!(
        matches!(err, ResolveError::ParseError(_, _)),
        "expected ParseError, got {err:?}"
    );
}

#[tokio::test]
async fn resolve_multi_level_chain_merges_all_parents() {
    let tmp = TempDir::new().unwrap();

    // chain: grandchild -> child -> root (vanilla).
    let mut root = empty_profile("1.20.1");
    root.main_class = Some("net.minecraft.client.main.Main".into());
    root.libraries = vec![lib("vanilla-lib")];
    write_profile(tmp.path(), &root);

    let mut child = empty_profile("1.20.1-forge-47.2.0");
    child.inherits_from = Some("1.20.1".into());
    child.libraries = vec![lib("forge-lib")];
    write_profile(tmp.path(), &child);

    let mut grandchild = empty_profile("1.20.1-forge-47.2.0-modpack");
    grandchild.inherits_from = Some("1.20.1-forge-47.2.0".into());
    grandchild.libraries = vec![lib("modpack-lib")];

    let resolved = resolve(grandchild, tmp.path()).await.unwrap();
    assert_eq!(resolved.id, "1.20.1-forge-47.2.0-modpack");
    assert!(resolved.inherits_from.is_none());
    assert_eq!(
        resolved.main_class.as_deref(),
        Some("net.minecraft.client.main.Main")
    );
    // libs: root ++ child ++ grandchild (each parent prepended)
    let names: Vec<_> = resolved.libraries.iter().map(|l| l.name.as_str()).collect();
    assert_eq!(names, vec!["vanilla-lib", "forge-lib", "modpack-lib"]);
}

#[tokio::test]
async fn resolve_detects_circular_chain() {
    let tmp = TempDir::new().unwrap();

    // a -> b -> a (cycle)
    let mut a = empty_profile("a");
    a.inherits_from = Some("b".into());
    write_profile(tmp.path(), &a);

    let mut b = empty_profile("b");
    b.inherits_from = Some("a".into());
    write_profile(tmp.path(), &b);

    // start from a fresh "a" profile that asks to inherit from b
    let mut entry = empty_profile("a");
    entry.inherits_from = Some("b".into());

    let err = resolve(entry, tmp.path()).await.unwrap_err();
    assert!(
        matches!(err, ResolveError::CircularInheritance(ref s) if s == "a"),
        "expected CircularInheritance(a), got {err:?}"
    );
}

#[tokio::test]
async fn resolve_caps_depth() {
    let tmp = TempDir::new().unwrap();

    // build a chain 0 -> 1 -> 2 -> ... -> 10. with cap of 8, hitting 10
    // should fail with DepthExceeded.
    for i in 0..=10 {
        let mut p = empty_profile(&format!("v{i}"));
        if i < 10 {
            p.inherits_from = Some(format!("v{}", i + 1));
        }
        write_profile(tmp.path(), &p);
    }

    let mut entry = empty_profile("entry");
    entry.inherits_from = Some("v0".into());

    let err = resolve(entry, tmp.path()).await.unwrap_err();
    assert!(
        matches!(err, ResolveError::DepthExceeded(_)),
        "expected DepthExceeded, got {err:?}"
    );
}
