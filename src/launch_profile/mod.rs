// foundation primitives for parsing, merging, and rendering mojang-format
// launch profiles. consumed by the vanilla launcher, forge/neoforge,
// fabric/quilt - anything that reads a mojang-style version JSON.
//
// phase 3 adds the `resolve` module that walks `inheritsFrom` chains.

pub mod model;
pub mod render;
pub mod resolve;
pub mod rules;
pub mod system;
pub mod templates;
