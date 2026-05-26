// resolves the `inheritsFrom` chain of a parsed `LaunchProfile`. mojang's
// version JSON allows a profile to inherit from another profile by id; the
// loaders (forge / neoforge / fabric / quilt) use this to layer their
// additions on top of a vanilla base. this module walks the chain and
// returns a single flat profile.
//
// merge semantics (per the mojang launcher and the major third-party
// launchers that interoperate with it):
//   - scalar fields: child wins if Some, else parent.
//   - libraries and arguments: parent ++ child (parent first). child
//     entries are appended after parent's.
//   - merge_into preserves parent's inherits_from so resolve() can keep
//     walking; resolve() clears the final result's inherits_from after
//     the loop exits.
//
// pure function `merge_into` handles the field-by-field merge math.
// async `resolve` does the chain walking with cycle detection and a depth
// cap. tests cover both layers independently.

use std::path::Path;

use super::model::{Arguments, LaunchProfile};

const MAX_INHERITANCE_DEPTH: usize = 8;

#[derive(Debug, thiserror::Error)]
pub enum ResolveError {
    #[error("parent profile not found at {0}")]
    ParentNotFound(String),
    #[error("failed to parse parent profile {0}: {1}")]
    ParseError(String, String),
    #[error("circular inheritance detected: {0} appears more than once in the chain")]
    CircularInheritance(String),
    #[error("inheritance chain exceeded {0} levels")]
    DepthExceeded(usize),
    #[error("I/O error reading parent profile: {0}")]
    Io(#[from] std::io::Error),
}

// merges `child` on top of `parent`. child takes precedence for scalar
// fields. for libraries and arguments, child entries are appended after
// parent's. `id` is taken from child. `inherits_from` is taken from
// parent (so resolve() can keep walking the chain - resolve() clears it
// to None after the final iteration).
pub fn merge_into(child: LaunchProfile, parent: LaunchProfile) -> LaunchProfile {
    LaunchProfile {
        id: child.id,
        inherits_from: parent.inherits_from,
        main_class: child.main_class.or(parent.main_class),
        libraries: merge_libraries(child.libraries, parent.libraries),
        arguments: merge_arguments(child.arguments, parent.arguments),
        minecraft_arguments: child.minecraft_arguments.or(parent.minecraft_arguments),
        asset_index: child.asset_index.or(parent.asset_index),
        assets: child.assets.or(parent.assets),
        java_version: child.java_version.or(parent.java_version),
        downloads: child.downloads.or(parent.downloads),
        release_time: child.release_time.or(parent.release_time),
        time: child.time.or(parent.time),
        game_arguments: None,
        type_: child.type_.or(parent.type_),
    }
}

// extracts the `group:artifact` portion of a maven coordinate, dropping
// version and any classifier. used as the dedup key when merging library
// lists from a child profile on top of its parent.
fn coord_key(name: &str) -> &str {
    // mojang maven coords are `group:artifact:version[:classifier]`. take
    // everything up to the second colon.
    let mut it = name.match_indices(':').map(|(i, _)| i);
    it.next();
    it.next().map_or(name, |i| &name[..i])
}

// child entries take precedence over parent entries with the same
// group:artifact. mojang and the major third-party launchers (prism,
// multimc) all dedup this way - without it, loader overrides of vanilla
// libraries (e.g. forge bumping log4j) would lose to vanilla because the
// JVM picks the first classpath match.
fn merge_libraries(
    child: Vec<crate::launch_profile::model::Library>,
    parent: Vec<crate::launch_profile::model::Library>,
) -> Vec<crate::launch_profile::model::Library> {
    use std::collections::HashSet;
    let child_keys: HashSet<&str> = child.iter().map(|l| coord_key(&l.name)).collect();

    let mut out: Vec<crate::launch_profile::model::Library> = parent
        .into_iter()
        .filter(|l| !child_keys.contains(coord_key(&l.name)))
        .collect();
    out.extend(child);
    out
}

fn merge_arguments(child: Option<Arguments>, parent: Option<Arguments>) -> Option<Arguments> {
    match (child, parent) {
        (None, None) => None,
        (Some(c), None) => Some(c),
        (None, Some(p)) => Some(p),
        (Some(c), Some(p)) => {
            let mut game = p.game;
            game.extend(c.game);
            let mut jvm = p.jvm;
            jvm.extend(c.jvm);
            Some(Arguments { game, jvm })
        }
    }
}

pub async fn resolve(
    profile: LaunchProfile,
    meta_dir: &Path,
) -> Result<LaunchProfile, ResolveError> {
    use std::collections::HashSet;

    let mut visited: HashSet<String> = HashSet::new();
    visited.insert(profile.id.clone());

    let mut current = profile;
    let mut depth = 0;

    while let Some(parent_id) = current.inherits_from.clone() {
        depth += 1;
        if depth > MAX_INHERITANCE_DEPTH {
            return Err(ResolveError::DepthExceeded(MAX_INHERITANCE_DEPTH));
        }
        if !visited.insert(parent_id.clone()) {
            return Err(ResolveError::CircularInheritance(parent_id));
        }

        let parent_path = meta_dir.join("versions").join(&parent_id).join("meta.json");
        if !parent_path.exists() {
            return Err(ResolveError::ParentNotFound(
                parent_path.display().to_string(),
            ));
        }
        let parent_bytes = tokio::fs::read(&parent_path).await?;
        let parent: LaunchProfile = serde_json::from_slice(&parent_bytes)
            .map_err(|e| ResolveError::ParseError(parent_id.clone(), e.to_string()))?;

        current = merge_into(current, parent);
    }

    current.inherits_from = None;
    Ok(current)
}

#[cfg(test)]
mod tests {
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
            .join("versions")
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

        let parent_path = tmp.path().join("versions").join("1.20.1").join("meta.json");
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
}
