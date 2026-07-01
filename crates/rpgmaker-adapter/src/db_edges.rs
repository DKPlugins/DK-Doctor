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

/// Actor: classId → Class; equips slot 0 → Weapon, slot 1 → Weapon when the actor
/// dual-wields (else Armor), slots ≥2 → Armor; traits.
///
/// RPG Maker resolves an equip id against Weapons vs Armors by the slot's *etype*,
/// not its index: normally only slot 0 is a weapon slot, but the Dual Wield trait
/// (`code 61` = TRAIT_SLOT_TYPE, `dataId 1`) turns slot 1 into a second weapon
/// slot, so `equips[1]` then holds a Weapon id. Mapping it positionally to Armor
/// would raise a false dangling-Armor reference and skip the real weapon check.
/// Only the actor's own traits are visible here (classes are parsed after actors),
/// so a class-granted dual wield is not detected — a conservative miss, never a
/// wrong-kind edge in the normal (non-dual-wield) case.
pub fn actor(b: &mut IrBuilder, from: EntityId, loc: &Location, rec: &Actor) {
    db_edge(b, from, loc, DbKind::Class, rec.class_id as i64);
    let dual_wield = rec.traits.iter().any(|t| t.code == 61 && t.data_id == 1);
    for (slot, &eq) in rec.equips.iter().enumerate() {
        // 0 = empty slot, skipped by db_edge; slot 0 (and slot 1 when dual-wielding)
        // → weapon, the rest → armor.
        let kind = if slot == 0 || (dual_wield && slot == 1) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::raw::database::Actor;
    use dk_doctor_core::ir::{DatabaseRecord, Engine, Entity, Ir};

    fn actor_rec(equips: Vec<i64>, traits: Vec<Trait>) -> Actor {
        Actor {
            id: 1,
            name: String::new(),
            class_id: 0,
            equips,
            face_name: String::new(),
            character_name: String::new(),
            battler_name: String::new(),
            traits,
        }
    }

    /// Weapon/Armor equip edges emitted for one actor record.
    fn equip_edges(rec: &Actor) -> Vec<(DbKind, u32)> {
        let mut b = Ir::builder(Engine::Mz);
        let loc = Location::file_only("data/Actors.json");
        let e = b.push_entity(
            Entity::DatabaseRecord(DatabaseRecord {
                kind: DbKind::Actor,
                record_id: 1,
                name: String::new(),
            }),
            loc.clone(),
        );
        actor(&mut b, e, &loc, rec);
        b.finish()
            .edges
            .iter()
            .filter_map(|r| match r.edge {
                Edge::ReferencesDbId { kind, id }
                    if matches!(kind, DbKind::Weapon | DbKind::Armor) =>
                {
                    Some((kind, id))
                }
                _ => None,
            })
            .collect()
    }

    #[test]
    fn slot_one_is_armor_normally_but_weapon_when_dual_wielding() {
        // Normal actor: slot 0 = weapon #1, slot 1 = shield/armor #2.
        let normal = equip_edges(&actor_rec(vec![1, 2, 0, 0, 0], vec![]));
        assert!(normal.contains(&(DbKind::Weapon, 1)));
        assert!(normal.contains(&(DbKind::Armor, 2)));
        assert!(!normal.contains(&(DbKind::Weapon, 2)));

        // Dual Wield (trait code 61, dataId 1): slot 1 holds an off-hand weapon,
        // so #2 must be checked against Weapons, not Armors.
        let dual = equip_edges(&actor_rec(
            vec![1, 2, 0, 0, 0],
            vec![Trait {
                code: 61,
                data_id: 1,
            }],
        ));
        assert!(dual.contains(&(DbKind::Weapon, 1)));
        assert!(
            dual.contains(&(DbKind::Weapon, 2)),
            "off-hand weapon must be a Weapon reference"
        );
        assert!(!dual.contains(&(DbKind::Armor, 2)));
    }
}
