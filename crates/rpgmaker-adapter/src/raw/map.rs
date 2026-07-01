//! `MapInfos.json` (sparse, index=mapId) and `MapXXX.json` (object with events/pages).

use crate::command::EventCommand;
use serde::Deserialize;

/// `MapInfos.json` entry — the authority on "which maps exist".
#[derive(Clone, Debug, Deserialize)]
pub struct MapInfo {
    /// Map id (== index).
    #[serde(default)]
    pub id: u32,
    /// Map name.
    #[serde(default)]
    pub name: String,
    /// Parent map id (0 = root).
    #[serde(default, rename = "parentId")]
    pub parent_id: u32,
}

/// `MapXXX.json` — a single map object.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct Map {
    /// Tileset id (img/tilesets/ indirectly).
    #[serde(default, rename = "tilesetId")]
    pub tileset_id: u32,
    /// Map width in tiles.
    #[serde(default)]
    pub width: u32,
    /// Map height in tiles.
    #[serde(default)]
    pub height: u32,
    /// Flat layered tile-id array (length = width*height*6). Layers z=0..3 are the
    /// tile layers used for passability; z=4 shadow, z=5 region. Empty on maps
    /// without saved tiles. Needed by the spatial (passability) analysis.
    #[serde(default)]
    pub data: Vec<i32>,
    /// battleback1Name (img/battlebacks1/).
    #[serde(default, rename = "battleback1Name")]
    pub battleback1_name: String,
    /// battleback2Name (img/battlebacks2/).
    #[serde(default, rename = "battleback2Name")]
    pub battleback2_name: String,
    /// parallaxName (img/parallaxes/).
    #[serde(default, rename = "parallaxName")]
    pub parallax_name: String,
    /// Map BGM autoplay.
    #[serde(default)]
    pub bgm: super::system::AudioFile,
    /// Map BGS autoplay.
    #[serde(default)]
    pub bgs: super::system::AudioFile,
    /// List of random encounters (`encounterList[].troopId` → Troops).
    #[serde(default, rename = "encounterList")]
    pub encounter_list: Vec<EncounterEntry>,
    /// Events (sparse array, null at 0 and in holes).
    #[serde(default)]
    pub events: Vec<Option<Event>>,
}

/// Map random-encounter entry (`encounterList[]`).
#[derive(Clone, Debug, Default, Deserialize)]
pub struct EncounterEntry {
    /// troopId → Troops.
    #[serde(default, rename = "troopId")]
    pub troop_id: u32,
}

/// Event on a map.
#[derive(Clone, Debug, Deserialize)]
pub struct Event {
    /// Event id (== index in events[]).
    #[serde(default)]
    pub id: u32,
    /// Event name.
    #[serde(default)]
    pub name: String,
    /// Event pages.
    #[serde(default)]
    pub pages: Vec<Page>,
}

/// Event page.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct Page {
    /// Activation conditions (switch/var read-sites).
    #[serde(default)]
    pub conditions: PageConditions,
    /// Event graphic (for the character asset reference).
    #[serde(default)]
    pub image: PageImage,
    /// Trigger (0 action..4 parallel).
    #[serde(default)]
    pub trigger: u32,
    /// Command list.
    #[serde(default)]
    pub list: Vec<EventCommand>,
}

/// Page conditions — an id is a read only when the `*Valid` flag is enabled.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct PageConditions {
    /// switch1Valid.
    #[serde(default, rename = "switch1Valid")]
    pub switch1_valid: bool,
    /// switch1Id.
    #[serde(default, rename = "switch1Id")]
    pub switch1_id: u32,
    /// switch2Valid.
    #[serde(default, rename = "switch2Valid")]
    pub switch2_valid: bool,
    /// switch2Id.
    #[serde(default, rename = "switch2Id")]
    pub switch2_id: u32,
    /// variableValid.
    #[serde(default, rename = "variableValid")]
    pub variable_valid: bool,
    /// variableId.
    #[serde(default, rename = "variableId")]
    pub variable_id: u32,
    /// variableValue (comparison threshold).
    #[serde(default, rename = "variableValue")]
    pub variable_value: i64,
    /// selfSwitchValid.
    #[serde(default, rename = "selfSwitchValid")]
    pub self_switch_valid: bool,
    /// selfSwitchCh ("A".."D").
    #[serde(default, rename = "selfSwitchCh")]
    pub self_switch_ch: String,
    /// itemValid.
    #[serde(default, rename = "itemValid")]
    pub item_valid: bool,
    /// itemId.
    #[serde(default, rename = "itemId")]
    pub item_id: u32,
    /// actorValid.
    #[serde(default, rename = "actorValid")]
    pub actor_valid: bool,
    /// actorId.
    #[serde(default, rename = "actorId")]
    pub actor_id: u32,
}

/// Event page graphic — character file name (img/characters/, ""=invisible).
#[derive(Clone, Debug, Default, Deserialize)]
pub struct PageImage {
    /// characterName.
    #[serde(default, rename = "characterName")]
    pub character_name: String,
    /// tileId (>0 — the event is drawn as a tile, not a sprite).
    #[serde(default, rename = "tileId")]
    pub tile_id: u32,
}
