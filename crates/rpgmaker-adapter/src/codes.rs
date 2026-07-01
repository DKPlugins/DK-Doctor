//! Constants for RPG Maker event command codes and small index helpers.
//!
//! This is the only place (together with [`crate::interpreter`]) where numeric
//! command codes live. The core does not know about them. Values are verified
//! against real MV/MZ data and the table in `docs/rpgmaker-format-spec.md` §1.

#![allow(missing_docs)]
// Some codes (102/115) are reserved for rules/detection in later stages and
// are not yet mapped in the interpreter.
#![allow(dead_code)]

// --- Messages / choices / control flow (§1.1) ---
pub const SHOW_TEXT: u16 = 101;
pub const SHOW_CHOICES: u16 = 102;
pub const INPUT_NUMBER: u16 = 103;
pub const SELECT_ITEM: u16 = 104;
/// Text body line that follows `101` Show Text (one per line).
pub const TEXT_DATA: u16 = 401;
/// Text body line that follows `105` Show Scrolling Text.
pub const SCROLL_TEXT_DATA: u16 = 405;
pub const GET_LOCATION_INFO: u16 = 285;
pub const CONDITIONAL_BRANCH: u16 = 111;
pub const LOOP: u16 = 112;
pub const EXIT_EVENT: u16 = 115;
pub const COMMON_EVENT: u16 = 117;
pub const REPEAT_ABOVE: u16 = 413;

// --- Switch / variable / self-switch (§1.3) ---
pub const CONTROL_SWITCHES: u16 = 121;
pub const CONTROL_VARIABLES: u16 = 122;
pub const CONTROL_SELF_SWITCH: u16 = 123;

// --- Inventory / actor / enemy (§1.4) ---
pub const CHANGE_GOLD: u16 = 125;
pub const CHANGE_ITEMS: u16 = 126;
pub const CHANGE_WEAPONS: u16 = 127;
pub const CHANGE_ARMORS: u16 = 128;
pub const CHANGE_PARTY_MEMBER: u16 = 129;
pub const NAME_INPUT: u16 = 303;
pub const CHANGE_HP: u16 = 311;
pub const CHANGE_MP: u16 = 312;
pub const CHANGE_STATE: u16 = 313;
pub const RECOVER_ALL: u16 = 314;
pub const CHANGE_EXP: u16 = 315;
pub const CHANGE_LEVEL: u16 = 316;
pub const CHANGE_PARAMETER: u16 = 317;
pub const CHANGE_SKILL: u16 = 318;
pub const CHANGE_EQUIPMENT: u16 = 319;
pub const CHANGE_NAME: u16 = 320;
pub const CHANGE_CLASS: u16 = 321;
pub const CHANGE_ACTOR_IMAGES: u16 = 322;
pub const CHANGE_VEHICLE_IMAGE: u16 = 323;
pub const CHANGE_NICKNAME: u16 = 324;
pub const CHANGE_PROFILE: u16 = 325;
pub const CHANGE_TP: u16 = 326;
pub const CHANGE_ENEMY_HP: u16 = 331;
pub const CHANGE_ENEMY_MP: u16 = 332;
pub const CHANGE_ENEMY_TP: u16 = 342;
pub const CHANGE_ENEMY_STATE: u16 = 333;
pub const ENEMY_TRANSFORM: u16 = 336;
pub const SHOW_BATTLE_ANIMATION: u16 = 337;

// --- Transfer / battle / shop (§1.5) ---
pub const TRANSFER_PLAYER: u16 = 201;
pub const SET_VEHICLE_LOCATION: u16 = 202;
pub const SET_EVENT_LOCATION: u16 = 203;
pub const SHOW_ANIMATION: u16 = 212;
pub const BATTLE_PROCESSING: u16 = 301;
pub const SHOP_PROCESSING: u16 = 302;
pub const SHOP_GOODS_ROW: u16 = 605;

// --- Pictures / media / map visuals (§1.6) ---
pub const SHOW_PICTURE: u16 = 231;
pub const MOVE_PICTURE: u16 = 232;
pub const ROTATE_PICTURE: u16 = 233;
pub const TINT_PICTURE: u16 = 234;
pub const ERASE_PICTURE: u16 = 235;
pub const PLAY_BGM: u16 = 241;
pub const PLAY_BGS: u16 = 245;
pub const PLAY_ME: u16 = 249;
pub const PLAY_SE: u16 = 250;
pub const PLAY_MOVIE: u16 = 261;
pub const CHANGE_TILESET: u16 = 282;
pub const CHANGE_BATTLE_BACK: u16 = 283;
pub const CHANGE_PARALLAX: u16 = 284;

// --- Scripts / plugin commands (§1.7) ---
pub const SCRIPT: u16 = 355;
pub const SCRIPT_CONT: u16 = 655;
pub const PLUGIN_COMMAND_MV: u16 = 356;
pub const PLUGIN_COMMAND_MZ: u16 = 357;

/// Target commands using the `iterateActorEx([0],[1])` convention (§1.4): `[0]==0` →
/// `[1]` is a literal actorId; `[0]==1` → `[1]` is a variableId (READ).
pub const ACTOR_EX_TARGET: &[u16] = &[
    CHANGE_HP,
    CHANGE_MP,
    CHANGE_STATE,
    RECOVER_ALL,
    CHANGE_EXP,
    CHANGE_LEVEL,
    CHANGE_PARAMETER,
    CHANGE_SKILL,
    CHANGE_TP,
];
