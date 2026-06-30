//! `System.json` — symbol table, asset fields, encryption flags, start map.

use serde::Deserialize;

/// Audio object `{name,volume,pitch,pan}` — only `name` is taken for asset references.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct AudioFile {
    /// File name without folder/extension (""=silence).
    #[serde(default)]
    pub name: String,
}

/// Vehicle description (boat/ship/airship) in System.json.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct Vehicle {
    /// Vehicle character graphic name (img/characters/).
    #[serde(default, rename = "characterName")]
    pub character_name: String,
    /// Vehicle BGM.
    #[serde(default)]
    pub bgm: AudioFile,
    /// Id of the map where the vehicle starts (0 = not set).
    #[serde(default, rename = "startMapId")]
    pub start_map_id: u32,
}

/// Parsed `System.json` (tolerant to extra fields and old MV).
#[derive(Clone, Debug, Default, Deserialize)]
pub struct System {
    /// Switch names (index 0 = "", length defines the id range).
    #[serde(default)]
    pub switches: Vec<Option<String>>,
    /// Variable names (likewise).
    #[serde(default)]
    pub variables: Vec<Option<String>>,
    /// Start map id.
    #[serde(default, rename = "startMapId")]
    pub start_map_id: u32,
    /// Id of the map opened in the editor (editMapId).
    #[serde(default, rename = "editMapId")]
    pub edit_map_id: u32,
    /// Whether images are encrypted.
    #[serde(default, rename = "hasEncryptedImages")]
    pub has_encrypted_images: bool,
    /// Whether audio is encrypted.
    #[serde(default, rename = "hasEncryptedAudio")]
    pub has_encrypted_audio: bool,
    /// Whether battle uses side view (for choosing the enemy folder).
    #[serde(default, rename = "optSideView")]
    pub opt_side_view: bool,

    /// title1Name (img/titles1/).
    #[serde(default, rename = "title1Name")]
    pub title1_name: String,
    /// title2Name (img/titles2/).
    #[serde(default, rename = "title2Name")]
    pub title2_name: String,
    /// battleback1Name of the battle test (img/battlebacks1/).
    #[serde(default, rename = "battleback1Name")]
    pub battleback1_name: String,
    /// battleback2Name of the battle test (img/battlebacks2/).
    #[serde(default, rename = "battleback2Name")]
    pub battleback2_name: String,
    /// Engine SE objects (sounds[24]).
    #[serde(default)]
    pub sounds: Vec<AudioFile>,
    /// Title screen BGM.
    #[serde(default, rename = "titleBgm")]
    pub title_bgm: AudioFile,
    /// Battle BGM.
    #[serde(default, rename = "battleBgm")]
    pub battle_bgm: AudioFile,
    /// Victory ME.
    #[serde(default, rename = "victoryMe")]
    pub victory_me: AudioFile,
    /// Defeat ME.
    #[serde(default, rename = "defeatMe")]
    pub defeat_me: AudioFile,
    /// Game over ME.
    #[serde(default, rename = "gameoverMe")]
    pub gameover_me: AudioFile,
    /// Boat.
    #[serde(default)]
    pub boat: Vehicle,
    /// Ship.
    #[serde(default)]
    pub ship: Vehicle,
    /// Airship.
    #[serde(default)]
    pub airship: Vehicle,

    /// MZ/new MV: extended block (its presence indicates the MZ era).
    #[serde(default)]
    pub advanced: Option<serde_json::Value>,
}

impl System {
    /// Maximum valid switch id (== `switches.len()-1`).
    pub fn max_switch_id(&self) -> u32 {
        self.switches.len().saturating_sub(1) as u32
    }

    /// Maximum valid variable id.
    pub fn max_variable_id(&self) -> u32 {
        self.variables.len().saturating_sub(1) as u32
    }
}
