//! Tier A plugin parsing: header annotations + reduction into IR facts.
//!
//! [`annotations`] parses the `/*: … */` header block of every ENABLED
//! plugin (agnostically, without knowledge of specific plugins). [`collect`]
//! merges the annotation schema with the `parameters` values from `plugins.js`
//! and populates the IR: switch/var declared by the plugin, provided assets,
//! the command registry, and load-order declarations.

pub mod annotations;
pub mod collect;
pub mod js;
