# contributing

thanks for wanting to contribute. here's how we/I do things around here.

## code style

### comments

preferred are `//` comments with a lowercase, casual tone. try to explain **why** something is done rather than restating what the code does. file-level comments are nice for describing what a file is responsible for, and function-level comments help when the behavior isn't obvious. enum variants and struct fields usually don't need comments if the names are clear enough. basically just look at how existing comments are written and match that vibe.

### visibility and naming

try to keep visibility as narrow as makes sense. `pub` when something is actually used outside the crate, `pub(crate)` across modules, `pub(super)` within a module. don't leave things `pub` just because they might be useful someday. function names should ideally be clear enough that you don't need a comment to explain what they do.

### modules and file organization

each file should have one clear responsibility. if a file starts doing two unrelated things, split it up. when a module gets big enough, convert `foo.rs` into `foo/mod.rs` with submodules. keep format-specific or protocol-specific code in its own file (e.g. `mrpack.rs` for modrinth, `mmc.rs` for multimc) and put shared types and dispatching in `mod.rs`. the general idea is that you shouldn't find multimc logic in the modrinth file or vice versa.

### tests

every test should cover a distinct code path. don't test the same branch twice with just different inputs. test names should describe the scenario, not just the function name. no need to test trivially correct code.

### general advice

- avoid "obvious" comments like `// save the profile` right above `save_profile()`
- avoid unnecessary abstractions or wrappers for things that only happen once
- avoid speculative features or "just in case" parameters
- prefer iterating directly over collecting into a vec when you only need one pass
- match existing error handling patterns, usually `map_err` with `format!`

## architecture

the codebase is split roughly like this:

- `src/cli/` handles command line interface
- `src/config/` has settings, paths, theme config
- `src/instance/` is the core. `content/` scans mods, resource packs, shaders, worlds. `import/` handles modpack importing with `mod.rs` dispatching to format-specific modules like `mrpack.rs` and `mmc.rs`. `loader/` installs mod loaders (fabric, forge, neoforge, quilt). `launch.rs` builds the java command and spawns minecraft. `manager.rs` does instance CRUD
- `src/net/` is the networking layer. http client, file downloads, and API clients per service
- `src/tui/` is the terminal UI built with ratatui

### adding a new import format

create a new file in `src/instance/import/`, add the variant to `PackFormat` in `mod.rs`, add detection logic in `detect_format()`, and add the dispatch arms in `build_summary()` and `execute_import()`.

### adding a new mod loader

create a new file in `src/instance/loader/` implementing `ModLoaderInstaller`, add the variant to `ModLoader` in `models.rs`, and register it in `get_installer()`.

## commits

start with what you did: `added`, `fixed`, `refactored`, etc. lowercase. separate logical changes into separate commits. don't bundle unrelated things into one commit.

## before submitting

make sure `cargo build`, `cargo clippy`, and `cargo test` all pass.
