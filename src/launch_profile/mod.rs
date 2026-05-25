// foundation primitives for parsing, merging, and rendering mojang-format
// launch profiles. consumed by the vanilla launcher, forge/neoforge,
// fabric/quilt - anything that reads a mojang-style version JSON.
//
// phase 1 is pure additions: no consumers yet. later phases will wire
// these primitives into the launch pipeline and the installer paths.

pub mod model;
pub mod rules;
pub mod templates;
