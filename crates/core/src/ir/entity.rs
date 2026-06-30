//! IR entities: vertices of the [`crate::ir::Ir::entities`] arena.
//!
//! Each [`EntityNode`] is a map, event, page, common event, enemy troop,
//! database record, asset, or opaque script. The core knows nothing about
//! RPG Maker commands; command codes live only in the adapter.

use crate::ir::asset::AssetRef;
use crate::ir::location::Location;

/// Entity identifier — index into the [`crate::ir::Ir::entities`] arena.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize)]
pub struct EntityId(pub u32);

/// Entity-arena vertex: id, content, and place in the project.
#[derive(Clone, Debug, serde::Serialize)]
pub struct EntityNode {
    /// Entity identifier.
    pub id: EntityId,
    /// Entity content.
    pub kind: Entity,
    /// Place of the entity in the project.
    pub location: Location,
}

/// Content of an IR entity.
#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "snake_case", tag = "entity")]
pub enum Entity {
    /// Map.
    Map(Map),
    /// Event on a map.
    Event(Event),
    /// Event page.
    Page(Page),
    /// Common event.
    CommonEvent(CommonEvent),
    /// Enemy troop (Troop).
    Troop(Troop),
    /// Database record.
    DatabaseRecord(DatabaseRecord),
    /// Reference to an asset as an entity.
    Asset(AssetRef),
    /// Opaque script block (355/655 / plugin body) — not parsed in iter1.
    Script(ScriptBlackbox),
    /// Plugin as an entity that is a source of references.
    ///
    /// Needed to attach edges produced by plugin parameter values from
    /// `plugins.js` (e.g. `@type common_event`/`@type state` → reference to a
    /// DB record, Tier A). A plugin has no place inside `data/`, so it is
    /// represented by a separate vertex located in `plugins.js`.
    Plugin(PluginRef),
}

/// Project map.
#[derive(Clone, Debug, serde::Serialize)]
pub struct Map {
    /// Map id.
    pub map_id: u32,
    /// Display name of the map.
    pub name: String,
    /// Event entities of this map.
    pub event_ids: Vec<EntityId>,
    /// Whether the map can start a battle: a non-empty `encounterList` or at
    /// least one command 301 (Battle Processing) in the map's events. Needed by
    /// usage-gating of battle-background assets (battlebacks).
    pub can_battle: bool,
}

/// Event on a map.
#[derive(Clone, Debug, serde::Serialize)]
pub struct Event {
    /// Id of the owning map.
    pub map_id: u32,
    /// Event id (== index in `events[]`).
    pub event_id: u32,
    /// Page entities of this event.
    pub page_ids: Vec<EntityId>,
}

/// Page of an event or common event.
///
/// Besides conditions and command count, the page **keeps the command
/// sequence** ([`Page::commands`]) with the code, indent, and place of each —
/// this is needed by the `dead-code-after-exit` rule so it doesn't re-read the
/// source.
#[derive(Clone, Debug, serde::Serialize)]
pub struct Page {
    /// Page activation conditions (switch/var read sites).
    pub conditions: PageConditions,
    /// Page activation trigger (how its command list is started).
    pub trigger: PageTrigger,
    /// Number of commands on the page.
    pub command_count: u32,
    /// Command sequence: code, indent, index, place.
    pub commands: Vec<CommandMeta>,
}

/// Event page activation trigger — how its command list is started.
///
/// Corresponds to the page's `trigger` field (0..4) in `MapXXX.json`. Common
/// events and troop pages have no trigger of their own — they use
/// [`CeTrigger`].
#[derive(Copy, Clone, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PageTrigger {
    /// 0 — started by a player action (button press).
    Action,
    /// 1 — player touch with the event.
    PlayerTouch,
    /// 2 — event touch with the player.
    EventTouch,
    /// 3 — autorun (Autorun).
    Autorun,
    /// 4 — parallel process (Parallel).
    Parallel,
}

impl PageTrigger {
    /// Converts a numeric page trigger (0..4) into a [`PageTrigger`];
    /// unknown values are treated as [`PageTrigger::Action`].
    pub fn from_raw(raw: u32) -> Self {
        match raw {
            1 => PageTrigger::PlayerTouch,
            2 => PageTrigger::EventTouch,
            3 => PageTrigger::Autorun,
            4 => PageTrigger::Parallel,
            _ => PageTrigger::Action,
        }
    }
}

/// Lightweight snapshot of a single list command — exactly as much as the
/// engine-independent rules need (finding unreachable code after an exit).
///
/// `code` itself is a number from the RPG Maker protocol, but the core does not
/// interpret its semantics; it merely matches codes (for example, the "exit
/// command"), which the adapter passes via [`crate::rules::RuleCtx`]-independent
/// constants. Only the number is stored here.
#[derive(Clone, Debug, serde::Serialize)]
pub struct CommandMeta {
    /// Numeric command code (engine protocol; the core does not interpret its semantics).
    pub code: u16,
    /// Indent level in the command list.
    pub indent: i32,
    /// Index of the command in the list (0-based).
    pub index: u32,
    /// Place of the command in the project.
    pub location: Location,
}

/// Common event (CommonEvent).
#[derive(Clone, Debug, serde::Serialize)]
pub struct CommonEvent {
    /// Id of the common event.
    pub id: u32,
    /// Name of the common event.
    pub name: String,
    /// Launch trigger.
    pub trigger: CeTrigger,
    /// Number of commands in the list (common events do not form [`Page`]
    /// entities, so the counter is stored here — for summary statistics).
    pub command_count: u32,
}

/// Enemy troop (Troop).
#[derive(Clone, Debug, serde::Serialize)]
pub struct Troop {
    /// Id of the troop.
    pub id: u32,
}

/// Database record (actor, item, skill, etc.).
#[derive(Clone, Debug, serde::Serialize)]
pub struct DatabaseRecord {
    /// Kind of DB record (determines the file).
    pub kind: DbKind,
    /// Record id (== index in the file's array).
    pub record_id: u32,
    /// Name of the record.
    pub name: String,
}

/// Opaque script block — body of commands 355/655 or plugin code.
///
/// Not parsed in iter1; kept for the future AST layer.
#[derive(Clone, Debug, serde::Serialize)]
pub struct ScriptBlackbox {
    /// Source text of the script.
    pub source: String,
}

/// Plugin as a graph vertex — a source of references from its parameter values.
#[derive(Clone, Debug, serde::Serialize)]
pub struct PluginRef {
    /// Plugin name (== `$plugins[].name` == name of the `.js`).
    pub name: String,
}

/// Kind of database record — determines the file and id namespace.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DbKind {
    /// Actors.json
    Actor,
    /// Classes.json
    Class,
    /// Skills.json
    Skill,
    /// Items.json
    Item,
    /// Weapons.json
    Weapon,
    /// Armors.json
    Armor,
    /// Enemies.json
    Enemy,
    /// Troops.json
    Troop,
    /// States.json
    State,
    /// Animations.json
    Animation,
    /// Tilesets.json
    Tileset,
    /// CommonEvents.json
    CommonEvent,
}

impl DbKind {
    /// DB file name without extension for this record kind.
    pub fn file_stem(self) -> &'static str {
        match self {
            DbKind::Actor => "Actors",
            DbKind::Class => "Classes",
            DbKind::Skill => "Skills",
            DbKind::Item => "Items",
            DbKind::Weapon => "Weapons",
            DbKind::Armor => "Armors",
            DbKind::Enemy => "Enemies",
            DbKind::Troop => "Troops",
            DbKind::State => "States",
            DbKind::Animation => "Animations",
            DbKind::Tileset => "Tilesets",
            DbKind::CommonEvent => "CommonEvents",
        }
    }
}

/// Launch trigger of a common event.
#[derive(Copy, Clone, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CeTrigger {
    /// Not started automatically (invoked by command 117).
    None,
    /// Autorun (Autorun).
    Autorun,
    /// Parallel process (Parallel).
    Parallel,
}

/// Event page activation conditions — these are switch/var **read sites**.
///
/// `Option::None` means the corresponding `*Valid` flag is off and the id is
/// not a read (even if a default value is present in the JSON).
#[derive(Clone, Debug, Default, serde::Serialize)]
pub struct PageConditions {
    /// switch1Id (if switch1Valid).
    pub switch1: Option<u32>,
    /// switch2Id (if switch2Valid).
    pub switch2: Option<u32>,
    /// variableId (if variableValid).
    pub variable: Option<u32>,
    /// variableValue — variable comparison threshold.
    pub variable_value: Option<i64>,
    /// selfSwitchCh (if selfSwitchValid).
    pub self_switch: Option<char>,
    /// itemId (if itemValid).
    pub item: Option<u32>,
    /// actorId (if actorValid).
    pub actor: Option<u32>,
}
