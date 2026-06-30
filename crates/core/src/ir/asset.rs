//! Asset references (images/audio/video/effects) — engine-independent.
//!
//! An asset reference is a "bare" name without folder or extension; the
//! specific folder is determined by [`AssetKind`]. Resolving to an on-disk
//! path and handling encryption is the adapter's job; the core sees only
//! normalized keys.

/// Asset key: kind (determines the folder) + bare name without extension.
#[derive(Clone, Debug, PartialEq, Eq, Hash, serde::Serialize)]
pub struct AssetKey {
    /// Asset kind — sets the folder and extension.
    pub kind: AssetKind,
    /// File name without folder and extension.
    pub name: String,
}

impl AssetKey {
    /// Creates an [`AssetKey`] from a kind and a name.
    pub fn new(kind: AssetKind, name: impl Into<String>) -> Self {
        Self {
            kind,
            name: name.into(),
        }
    }
}

/// Alias for [`AssetKey`], used as the asset entity in the IR.
pub type AssetRef = AssetKey;

/// Asset kind — unambiguously sets the folder (and extension) on disk.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AssetKind {
    /// `img/faces/`
    Face,
    /// `img/characters/`
    Character,
    /// `img/pictures/`
    Picture,
    /// `img/parallaxes/`
    Parallax,
    /// `img/tilesets/`
    Tileset,
    /// `img/battlebacks1/`
    Battleback1,
    /// `img/battlebacks2/`
    Battleback2,
    /// `img/titles1/`
    Title1,
    /// `img/titles2/`
    Title2,
    /// `img/enemies/`
    Enemy,
    /// `img/sv_enemies/`
    SvEnemy,
    /// `img/sv_actors/`
    SvActor,
    /// `img/animations/`
    Animation,
    /// `effects/` (Effekseer, MZ)
    Effect,
    /// `movies/`
    Movie,
    /// `audio/bgm/`
    Bgm,
    /// `audio/bgs/`
    Bgs,
    /// `audio/me/`
    Me,
    /// `audio/se/`
    Se,
}

impl AssetKind {
    /// Standard on-disk folder for this kind (no trailing slash), e.g. `img/pictures`.
    ///
    /// Single source of truth for the kind→folder mapping shared by the asset
    /// rules and the plugin profiles (a language-neutral path identifier).
    pub fn folder(self) -> &'static str {
        match self {
            AssetKind::Face => "img/faces",
            AssetKind::Character => "img/characters",
            AssetKind::Picture => "img/pictures",
            AssetKind::Parallax => "img/parallaxes",
            AssetKind::Tileset => "img/tilesets",
            AssetKind::Battleback1 => "img/battlebacks1",
            AssetKind::Battleback2 => "img/battlebacks2",
            AssetKind::Title1 => "img/titles1",
            AssetKind::Title2 => "img/titles2",
            AssetKind::Enemy => "img/enemies",
            AssetKind::SvEnemy => "img/sv_enemies",
            AssetKind::SvActor => "img/sv_actors",
            AssetKind::Animation => "img/animations",
            AssetKind::Effect => "effects",
            AssetKind::Movie => "movies",
            AssetKind::Bgm => "audio/bgm",
            AssetKind::Bgs => "audio/bgs",
            AssetKind::Me => "audio/me",
            AssetKind::Se => "audio/se",
        }
    }

    /// Asset kind from a standard folder label (inverse of [`AssetKind::folder`]).
    /// Tolerant to surrounding slashes; `None` for an unknown/non-standard label.
    pub fn from_folder(folder: &str) -> Option<Self> {
        Some(match folder.trim_matches('/') {
            "img/faces" => AssetKind::Face,
            "img/characters" => AssetKind::Character,
            "img/pictures" => AssetKind::Picture,
            "img/parallaxes" => AssetKind::Parallax,
            "img/tilesets" => AssetKind::Tileset,
            "img/battlebacks1" => AssetKind::Battleback1,
            "img/battlebacks2" => AssetKind::Battleback2,
            "img/titles1" => AssetKind::Title1,
            "img/titles2" => AssetKind::Title2,
            "img/enemies" => AssetKind::Enemy,
            "img/sv_enemies" => AssetKind::SvEnemy,
            "img/sv_actors" => AssetKind::SvActor,
            "img/animations" => AssetKind::Animation,
            "effects" => AssetKind::Effect,
            "movies" => AssetKind::Movie,
            "audio/bgm" => AssetKind::Bgm,
            "audio/bgs" => AssetKind::Bgs,
            "audio/me" => AssetKind::Me,
            "audio/se" => AssetKind::Se,
            _ => return None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folder_round_trips_for_every_kind() {
        for kind in [
            AssetKind::Face,
            AssetKind::Character,
            AssetKind::Picture,
            AssetKind::Parallax,
            AssetKind::Tileset,
            AssetKind::Battleback1,
            AssetKind::Battleback2,
            AssetKind::Title1,
            AssetKind::Title2,
            AssetKind::Enemy,
            AssetKind::SvEnemy,
            AssetKind::SvActor,
            AssetKind::Animation,
            AssetKind::Effect,
            AssetKind::Movie,
            AssetKind::Bgm,
            AssetKind::Bgs,
            AssetKind::Me,
            AssetKind::Se,
        ] {
            assert_eq!(AssetKind::from_folder(kind.folder()), Some(kind));
        }
        assert_eq!(AssetKind::from_folder("img/nonsense"), None);
    }
}
