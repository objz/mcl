# Managed mod dependencies

## Summary

Discovery installs should understand provider-declared mod dependencies. Modrinth
and CurseForge dependency metadata is normalized into one internal model, while
modpacks continue to use their existing pack manifests and resource packs and
shaders remain unchanged.

## Dependency handling

- Recursively resolve required dependencies before downloading anything.
- For project-only requirements, select the newest compatible stable release for
  the instance's Minecraft version and loader, falling back to beta and then
  alpha.
- Honor exact Modrinth version requirements and fail clearly when the version is
  incompatible.
- Deduplicate shared dependencies and reject cycles, incompatible installed
  projects, and conflicting exact-version requirements.
- Keep an installed dependency unless the provider requires another exact
  version.
- Recognize dependencies installed from the other provider only through an exact
  fingerprint match.
- Do not install optional dependencies. Ignore embedded or included files because
  they are already packaged.
- Resolve the complete operation before downloading, stage every download, and
  update files and the content manifest together. Failed operations must restore
  replaced files and remove staged downloads.
- Extend the existing install confirmation with the dependencies that will be
  installed and any installed dependencies that will be replaced.

## Tracking, updates, and removal

- Store required dependency edges, exact provider aliases, and whether a file was
  installed automatically in the content manifest with backward-compatible
  defaults.
- Existing files remain user-managed. Directly installing or changing an
  automatic dependency promotes it to user-managed content.
- Recalculate dependencies when a parent mod changes version.
- Keep the normal content-delete confirmation. If the removal leaves
  automatically installed dependencies with no remaining dependents, only
  include projects categorized exclusively as libraries. Missing categories or
  any additional functional category keep the dependency installed.
- Keep shared dependencies while any installed mod requires them.
- Warn and require a stronger confirmation when directly removing a dependency
  that installed mods still require, but allow the user to continue.

## Provider and UI changes

- Add normalized dependency relations and release type to the provider-neutral
  version model.
- Add exact-version lookup to the provider interface.
- Run dependency resolution while the existing version popup is in its loading
  state, then reuse its confirmation view for the installation summary.
- Show optional dependency counts without presenting them as installed content.
- Keep installation progress in the normal overview status area.

## Tests

- Provider parsing for every dependency relation and release channel.
- Transitive dependencies, shared libraries, cycles, conflicting versions,
  incompatible projects, installed matches, and cross-provider fingerprints.
- Backward-compatible manifest loading, explicit promotion, updates, shared
  references, and orphan cleanup.
- Staged installation rollback and successful multi-file commits.
- TUI interaction and snapshots for dependency summaries, replacements, orphan
  cleanup, and required-library warnings.
- Focused tests, the full deterministic suite, Clippy with warnings denied, and
  the repository snapshot check.

## Discovery roadmap

- [x] Managed mods, resource packs, and shaders
- [x] Modrinth and built-in CurseForge discovery
- [x] Modpack browsing and import
- [x] Search, endless scrolling, and page jumping
- [x] Provider project pages and version selection
- [x] Managed mod dependencies
- [x] Datapack discovery
- [x] Compatible update checks for managed content and discovered modpacks
