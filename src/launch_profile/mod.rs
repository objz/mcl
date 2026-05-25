// foundation primitives for parsing, merging, and rendering mojang-format
// launch profiles. consumed by the vanilla launcher, forge/neoforge,
// fabric/quilt - anything that reads a mojang-style version JSON.
//
// phase 2 adds the render layer that turns a parsed profile + rule context +
// template context into a flat argv list.

pub mod model;
pub mod render;
pub mod rules;
pub mod templates;
