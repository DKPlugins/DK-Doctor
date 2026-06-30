//! Tolerant serde structs for `data/*.json` and `js/plugins.js`.
//!
//! Principles (see `docs/architecture.md` §3): we type only the analytical
//! "backbone" — id/name and the needed FK fields; everything optional is marked
//! `#[serde(default)]`; **never** `deny_unknown_fields` (plugins
//! add their own fields). DB tables are `Vec<Option<T>>` (index == id,
//! `null` at 0 and in holes). Command payloads stay `Vec<Value>`.
//!
//! Some fields (FK for referential-integrity from traits/effects/equips,
//! encryption flags, plugin status) are not read by the code yet — they are
//! captured ahead of time as an analytical backbone for the rules of later stages.
#![allow(dead_code)]

pub mod common_event;
pub mod database;
pub mod map;
pub mod plugins;
pub mod system;
