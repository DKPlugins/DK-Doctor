//! Database tables: Actors/Classes/Skills/Items/Weapons/Armors/Enemies/
//! Troops/States/Animations/Tilesets.
//!
//! Each table is a `Vec<Option<T>>` (index == id, `null` at 0 and in holes).
//! Captures `id`+`name` and FK fields from `docs/rpgmaker-format-spec.md` §3.
//! Extensions/notetags are not typed.

use serde::Deserialize;

/// `traits[]` = `{code,dataId,value}`; `dataId` is a typed FK keyed by `code`.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct Trait {
    /// Trait code.
    #[serde(default)]
    pub code: u32,
    /// Target id (meaning depends on `code`).
    #[serde(default, rename = "dataId")]
    pub data_id: i64,
}

/// `effects[]` = `{code,dataId,value1,value2}` (Skills/Items).
#[derive(Clone, Debug, Default, Deserialize)]
pub struct Effect {
    /// Effect code.
    #[serde(default)]
    pub code: u32,
    /// Target id (meaning depends on `code`).
    #[serde(default, rename = "dataId")]
    pub data_id: i64,
}

/// Damage block of a skill/item (`damage.elementId`).
#[derive(Clone, Debug, Default, Deserialize)]
pub struct Damage {
    /// Element id (-1 normal, 0 none).
    #[serde(default, rename = "elementId")]
    pub element_id: i64,
}

/// Actors.json.
#[derive(Clone, Debug, Deserialize)]
pub struct Actor {
    /// Id.
    #[serde(default)]
    pub id: u32,
    /// Name.
    #[serde(default)]
    pub name: String,
    /// classId → Classes.
    #[serde(default, rename = "classId")]
    pub class_id: u32,
    /// equips[] (slot 0 → Weapons, slot 1 → Weapons when dual-wielding else
    /// Armors, the rest → Armors; 0=empty). See [`crate::db_edges::actor`].
    #[serde(default)]
    pub equips: Vec<i64>,
    /// faceName (img/faces/).
    #[serde(default, rename = "faceName")]
    pub face_name: String,
    /// characterName (img/characters/).
    #[serde(default, rename = "characterName")]
    pub character_name: String,
    /// battlerName (img/sv_actors/).
    #[serde(default, rename = "battlerName")]
    pub battler_name: String,
    /// traits.
    #[serde(default)]
    pub traits: Vec<Trait>,
}

/// Classes.json.
#[derive(Clone, Debug, Deserialize)]
pub struct Class {
    /// Id.
    #[serde(default)]
    pub id: u32,
    /// Name.
    #[serde(default)]
    pub name: String,
    /// learnings[].skillId → Skills.
    #[serde(default)]
    pub learnings: Vec<Learning>,
    /// traits.
    #[serde(default)]
    pub traits: Vec<Trait>,
}

/// A record of a skill learned by a class.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct Learning {
    /// skillId → Skills.
    #[serde(default, rename = "skillId")]
    pub skill_id: u32,
}

/// Skills.json.
#[derive(Clone, Debug, Deserialize)]
pub struct Skill {
    /// Id.
    #[serde(default)]
    pub id: u32,
    /// Name.
    #[serde(default)]
    pub name: String,
    /// animationId (-1 normal attack, 0 none).
    #[serde(default, rename = "animationId")]
    pub animation_id: i64,
    /// damage.elementId.
    #[serde(default)]
    pub damage: Damage,
    /// effects.
    #[serde(default)]
    pub effects: Vec<Effect>,
}

/// Items.json.
#[derive(Clone, Debug, Deserialize)]
pub struct Item {
    /// Id.
    #[serde(default)]
    pub id: u32,
    /// Name.
    #[serde(default)]
    pub name: String,
    /// animationId.
    #[serde(default, rename = "animationId")]
    pub animation_id: i64,
    /// damage.elementId.
    #[serde(default)]
    pub damage: Damage,
    /// effects.
    #[serde(default)]
    pub effects: Vec<Effect>,
}

/// Weapons.json.
#[derive(Clone, Debug, Deserialize)]
pub struct Weapon {
    /// Id.
    #[serde(default)]
    pub id: u32,
    /// Name.
    #[serde(default)]
    pub name: String,
    /// animationId.
    #[serde(default, rename = "animationId")]
    pub animation_id: i64,
    /// traits.
    #[serde(default)]
    pub traits: Vec<Trait>,
}

/// Armors.json.
#[derive(Clone, Debug, Deserialize)]
pub struct Armor {
    /// Id.
    #[serde(default)]
    pub id: u32,
    /// Name.
    #[serde(default)]
    pub name: String,
    /// traits.
    #[serde(default)]
    pub traits: Vec<Trait>,
}

/// Enemies.json.
#[derive(Clone, Debug, Deserialize)]
pub struct Enemy {
    /// Id.
    #[serde(default)]
    pub id: u32,
    /// Name.
    #[serde(default)]
    pub name: String,
    /// battlerName (img/enemies/ or img/sv_enemies/).
    #[serde(default, rename = "battlerName")]
    pub battler_name: String,
    /// actions[].
    #[serde(default)]
    pub actions: Vec<EnemyAction>,
    /// dropItems[].
    #[serde(default, rename = "dropItems")]
    pub drop_items: Vec<DropItem>,
    /// traits.
    #[serde(default)]
    pub traits: Vec<Trait>,
}

/// Enemy action in battle.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct EnemyAction {
    /// skillId → Skills.
    #[serde(default, rename = "skillId")]
    pub skill_id: u32,
    /// Condition type (4 → States, 6 → System.switches).
    #[serde(default, rename = "conditionType")]
    pub condition_type: u32,
    /// Condition parameter (meaning depends on the type).
    #[serde(default, rename = "conditionParam1")]
    pub condition_param1: i64,
}

/// Enemy drop `{kind,dataId}`: kind 1→Items, 2→Weapons, 3→Armors.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct DropItem {
    /// Kind (1 item / 2 weapon / 3 armor).
    #[serde(default)]
    pub kind: u32,
    /// Target id by kind (i64 like the other FK fields: tolerates sentinel
    /// values such as -1 without aborting the whole table parse).
    #[serde(default, rename = "dataId")]
    pub data_id: i64,
}

/// States.json.
#[derive(Clone, Debug, Deserialize)]
pub struct State {
    /// Id.
    #[serde(default)]
    pub id: u32,
    /// Name.
    #[serde(default)]
    pub name: String,
    /// traits (states carry traits, like the other records).
    #[serde(default)]
    pub traits: Vec<Trait>,
}

/// Tilesets.json.
#[derive(Clone, Debug, Deserialize)]
pub struct Tileset {
    /// Id.
    #[serde(default)]
    pub id: u32,
    /// Name.
    #[serde(default)]
    pub name: String,
    /// tilesetNames[9] (slots A1..E; ""=unused) → img/tilesets/.
    #[serde(default, rename = "tilesetNames")]
    pub tileset_names: Vec<String>,
    /// Per-tile passage flags (indexed by tile id, length up to 0x2000). Bits 0..3
    /// are the four impassable directions (0x0f = impassable), 0x10 = star
    /// (passable, drawn above). Used by the spatial (passability) analysis.
    #[serde(default)]
    pub flags: Vec<u32>,
}

/// Animations.json — MV-style (`frames`) or MZ-Effekseer (`effectName`).
#[derive(Clone, Debug, Deserialize)]
pub struct Animation {
    /// Id.
    #[serde(default)]
    pub id: u32,
    /// Name.
    #[serde(default)]
    pub name: String,
    /// MV-style frames (their presence distinguishes the MV format from Effekseer).
    #[serde(default)]
    pub frames: Option<serde_json::Value>,
    /// animation1Name (MV, img/animations/).
    #[serde(default, rename = "animation1Name")]
    pub animation1_name: String,
    /// animation2Name (MV, img/animations/).
    #[serde(default, rename = "animation2Name")]
    pub animation2_name: String,
    /// effectName (MZ Effekseer, effects/*.efkefc).
    #[serde(default, rename = "effectName")]
    pub effect_name: String,
}

/// Troops.json.
#[derive(Clone, Debug, Deserialize)]
pub struct Troop {
    /// Id.
    #[serde(default)]
    pub id: u32,
    /// Name.
    #[serde(default)]
    pub name: String,
    /// members[].
    #[serde(default)]
    pub members: Vec<TroopMember>,
    /// pages[].
    #[serde(default)]
    pub pages: Vec<TroopPage>,
}

/// Member of an enemy troop.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct TroopMember {
    /// enemyId → Enemies.
    #[serde(default, rename = "enemyId")]
    pub enemy_id: u32,
}

/// Page of an enemy troop.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct TroopPage {
    /// Page conditions.
    #[serde(default)]
    pub conditions: TroopPageConditions,
    /// Command list.
    #[serde(default)]
    pub list: Vec<crate::command::EventCommand>,
}

/// Conditions of an enemy troop page.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct TroopPageConditions {
    /// switchValid.
    #[serde(default, rename = "switchValid")]
    pub switch_valid: bool,
    /// switchId (READ when switchValid) → System.switches.
    #[serde(default, rename = "switchId")]
    pub switch_id: u32,
    /// actorValid.
    #[serde(default, rename = "actorValid")]
    pub actor_valid: bool,
    /// actorId (READ when actorValid) → Actors.
    #[serde(default, rename = "actorId")]
    pub actor_id: u32,
}

/// Parses a 1-based DB table into `Vec<Option<T>>`, resilient to garbage records.
///
/// The outer array is parsed as raw `Value`s, then each record is deserialized
/// independently: a record that fails (an unexpected `null` in a captured field,
/// an out-of-range number, an unexpected shape from a third-party tool) becomes
/// `None` and is skipped, instead of aborting the whole table — which would
/// silently erase every record and blind every rule that depends on it. Only a
/// malformed outer array (not a JSON array at all) returns `Err`.
pub fn parse_table<T: serde::de::DeserializeOwned>(
    bytes: &str,
) -> Result<Vec<Option<T>>, serde_json::Error> {
    let raw: Vec<Option<serde_json::Value>> = serde_json::from_str(bytes)?;
    Ok(raw
        .into_iter()
        .map(|slot| slot.and_then(|v| serde_json::from_value::<T>(v).ok()))
        .collect())
}
