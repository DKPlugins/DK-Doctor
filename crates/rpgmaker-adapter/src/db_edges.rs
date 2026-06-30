//! Emission of data-side referential edges (`Edge::ReferencesDbId`) from parsed
//! DB records — strictly per `docs/rpgmaker-format-spec.md` §3/§3.2/§3.3.
//!
//! Each function receives the record's `EntityId` and its [`Location`] and pushes
//! an edge for every FK (id==0 and negatives = "none", skipped). There are no
//! numeric command codes here — only static FK fields of DB files. Trait codes
//! are mapped only to DB-file targets; System-type codes are skipped
//! (see §3.2) and marked as a follow-up.

use crate::raw::database::{Actor, Armor, Class, Enemy, Item, Skill, State, Trait, Weapon};
use dk_doctor_core::ir::{DbKind, Edge, EntityId, IrBuilder, Location};

/// Pushes `ReferencesDbId{kind,id}` if `id` is positive (>0).
fn db_edge(b: &mut IrBuilder, from: EntityId, loc: &Location, kind: DbKind, id: i64) {
    if id > 0 {
        b.push_edge(
            from,
            Edge::ReferencesDbId {
                kind,
                id: id as u32,
            },
            loc.clone(),
        );
    }
}

/// Maps `traits[].dataId` to a DB-file target by `code` (§3.2).
///
/// Returns `None` for System-type codes (11/31/41/42/51/52/53/54) and
/// for non-referential codes — those traits do not point to DB files.
/// **Follow-up:** System-type targets (elements/skillTypes/…) are not yet
/// checked for existence, since they are not [`DbKind`] files.
fn trait_db_target(code: u32) -> Option<DbKind> {
    match code {
        13 | 14 | 32 => Some(DbKind::State), // STATE_RATE / STATE_RESIST / ATTACK_STATE
        35 | 43 | 44 => Some(DbKind::Skill), // ATTACK_SKILL / SKILL_ADD / SKILL_SEAL
        _ => None,
    }
}

/// Emits edges for the record's DB-file trait targets (§3.2).
fn emit_trait_edges(b: &mut IrBuilder, from: EntityId, loc: &Location, traits: &[Trait]) {
    for t in traits {
        if let Some(kind) = trait_db_target(t.code) {
            db_edge(b, from, loc, kind, t.data_id);
        }
    }
}

/// Maps `effects[].dataId` to a DB-file target by `code` (§3.3).
///
/// 21/22 → State, 43 → Skill, 44 → CommonEvent. Others are non-referential.
fn emit_effect_edges(
    b: &mut IrBuilder,
    from: EntityId,
    loc: &Location,
    effects: &[crate::raw::database::Effect],
) {
    for e in effects {
        let kind = match e.code {
            21 | 22 => Some(DbKind::State),
            43 => Some(DbKind::Skill),
            44 => Some(DbKind::CommonEvent),
            _ => None,
        };
        if let Some(kind) = kind {
            db_edge(b, from, loc, kind, e.data_id);
        }
    }
}

/// Actor: classId → Class; equips slot 0 → Weapon, slots ≥1 → Armor; traits.
pub fn actor(b: &mut IrBuilder, from: EntityId, loc: &Location, rec: &Actor) {
    db_edge(b, from, loc, DbKind::Class, rec.class_id as i64);
    for (slot, &eq) in rec.equips.iter().enumerate() {
        // 0 = empty slot, skip; slot 0 → weapon, the rest → armor.
        let kind = if slot == 0 {
            DbKind::Weapon
        } else {
            DbKind::Armor
        };
        db_edge(b, from, loc, kind, eq);
    }
    emit_trait_edges(b, from, loc, &rec.traits);
}

/// Class: learnings[].skillId → Skill; traits.
pub fn class(b: &mut IrBuilder, from: EntityId, loc: &Location, rec: &Class) {
    for l in &rec.learnings {
        db_edge(b, from, loc, DbKind::Skill, l.skill_id as i64);
    }
    emit_trait_edges(b, from, loc, &rec.traits);
}

/// Skill: animationId (>0) → Animation; effects (21/22 State, 43 Skill, 44 CE).
pub fn skill(b: &mut IrBuilder, from: EntityId, loc: &Location, rec: &Skill) {
    db_edge(b, from, loc, DbKind::Animation, rec.animation_id);
    emit_effect_edges(b, from, loc, &rec.effects);
}

/// Item: animationId (>0) → Animation; effects (same mapping).
pub fn item(b: &mut IrBuilder, from: EntityId, loc: &Location, rec: &Item) {
    db_edge(b, from, loc, DbKind::Animation, rec.animation_id);
    emit_effect_edges(b, from, loc, &rec.effects);
}

/// Weapon: animationId (>0) → Animation; traits.
pub fn weapon(b: &mut IrBuilder, from: EntityId, loc: &Location, rec: &Weapon) {
    db_edge(b, from, loc, DbKind::Animation, rec.animation_id);
    emit_trait_edges(b, from, loc, &rec.traits);
}

/// Armor: traits.
pub fn armor(b: &mut IrBuilder, from: EntityId, loc: &Location, rec: &Armor) {
    emit_trait_edges(b, from, loc, &rec.traits);
}

/// State: traits.
pub fn state(b: &mut IrBuilder, from: EntityId, loc: &Location, rec: &State) {
    emit_trait_edges(b, from, loc, &rec.traits);
}

/// Enemy: actions[].skillId → Skill; condition_type==4 → State;
/// dropItems kind 1/2/3 → Item/Weapon/Armor (dataId>0); traits.
pub fn enemy(b: &mut IrBuilder, from: EntityId, loc: &Location, rec: &Enemy) {
    for a in &rec.actions {
        db_edge(b, from, loc, DbKind::Skill, a.skill_id as i64);
        if a.condition_type == 4 {
            db_edge(b, from, loc, DbKind::State, a.condition_param1);
        }
    }
    for d in &rec.drop_items {
        let kind = match d.kind {
            1 => Some(DbKind::Item),
            2 => Some(DbKind::Weapon),
            3 => Some(DbKind::Armor),
            _ => None,
        };
        if let Some(kind) = kind {
            db_edge(b, from, loc, kind, d.data_id);
        }
    }
    emit_trait_edges(b, from, loc, &rec.traits);
}
