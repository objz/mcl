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

        let parent_path = crate::storage::MetadataPaths::new(meta_dir)
            .versions()
            .join(&parent_id)
            .join("meta.json");
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
#[path = "tests/resolve.rs"]
mod tests;
