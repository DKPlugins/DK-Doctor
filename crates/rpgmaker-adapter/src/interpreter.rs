//! Command list interpreter — mapping command codes to IR edges and symbol
//! sites **strictly** by the index table in `docs/rpgmaker-format-spec.md`.
//!
//! This is the only place where numeric command codes are turned into engine-
//! independent edges/sites. The walk does not run the game: it merely reads the
//! positional parameters of each command. Indices verified against real MV/MZ data.

use crate::codes;
use crate::command::EventCommand;
use dk_doctor_core::ir::{
    AssetKey, AssetKind, CmpOp, DbKind, DeadBranch, Edge, EntityId, IrBuilder, Location, PathSeg,
    PluginCommandCall, SelfSwitchKey, Site, SwitchGate, TransferDesignation,
};
use std::collections::HashMap;

/// An inclusive integer range `[lo, hi]` a variable is statically known to hold.
/// An exact constant is the degenerate range `lo == hi`. All arithmetic saturates
/// so a malformed/huge operand cannot overflow.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
struct Interval {
    lo: i64,
    hi: i64,
}

impl Interval {
    /// The exact value `v` as a one-point range.
    fn exact(v: i64) -> Self {
        Self { lo: v, hi: v }
    }

    /// A range from a raw `(a, b)` pair, normalized so `lo <= hi`.
    fn range(a: i64, b: i64) -> Self {
        Self {
            lo: a.min(b),
            hi: a.max(b),
        }
    }

    /// `self + other` (interval addition, saturating).
    fn add(self, other: Interval) -> Interval {
        Interval {
            lo: self.lo.saturating_add(other.lo),
            hi: self.hi.saturating_add(other.hi),
        }
    }

    /// `self - other` (interval subtraction, saturating).
    fn sub(self, other: Interval) -> Interval {
        Interval {
            lo: self.lo.saturating_sub(other.hi),
            hi: self.hi.saturating_sub(other.lo),
        }
    }
}

/// Lightweight **symbolic-range** propagation of variable values **within a single
/// command list**: tracks the range each variable can hold via command 122
/// (Control Variables: set / add / sub / random) while honoring indent dominance.
/// This underpins: resolution of by-variable transfers (201) / battles (301) —
/// which use the exact case (`lo == hi`) — and constant-resolvable conditions
/// (111), which use the full range (`var >= K` is dead when the whole range is
/// below `K`).
///
/// Dominance: a value assigned at indent `L` is visible to subsequent commands
/// while their indent is `>= L`; on leaving the block (indent drops below `L`) the
/// assignment stops dominating and is removed ([`ConstEnv::prune`]). Opaque
/// commands (common event call, plugin command, script) may change any variables
/// outside our visibility — after them the environment is wholly reset
/// ([`ConstEnv::clear`]). Operations we cannot bound (mul/div/mod, game data)
/// invalidate the target. This is deliberately conservative: skipping yields fewer
/// findings, but not false ones.
#[derive(Default)]
struct ConstEnv {
    /// `varId` → (value range, indent of the assigning command).
    vals: HashMap<u32, (Interval, i32)>,
}

impl ConstEnv {
    /// Removes ranges assigned at a deeper indent than `indent`
    /// (left their block — the assignment no longer dominates the current command).
    fn prune(&mut self, indent: i32) {
        self.vals
            .retain(|_, &mut (_, set_indent)| set_indent <= indent);
    }

    /// Full reset (an opaque command may have changed any variables).
    fn clear(&mut self) {
        self.vals.clear();
    }

    /// Records a known value range of a variable.
    fn set(&mut self, var: u32, value: Interval, indent: i32) {
        if var != 0 {
            self.vals.insert(var, (value, indent));
        }
    }

    /// Discards knowledge of a variable's value (an unknown value was assigned).
    fn invalidate(&mut self, var: u32) {
        self.vals.remove(&var);
    }

    /// The currently known value range of a variable.
    fn get(&self, var: u32) -> Option<Interval> {
        self.vals.get(&var).map(|&(v, _)| v)
    }

    /// The currently known **exact** value of a variable (`lo == hi`) — for
    /// by-variable transfer/battle resolution, which needs a single map/troop id.
    fn get_exact(&self, var: u32) -> Option<i64> {
        self.get(var).filter(|iv| iv.lo == iv.hi).map(|iv| iv.lo)
    }
}

/// Whether `lhs <op> rhs` is guaranteed over the two ranges: `Some(true)` when it
/// holds for every pair, `Some(false)` when it holds for no pair, `None` when the
/// result depends on the concrete values (not statically decidable).
fn interval_cmp_definite(lhs: Interval, op: CmpOp, rhs: Interval) -> Option<bool> {
    let disjoint = lhs.hi < rhs.lo || rhs.hi < lhs.lo;
    let single_equal = lhs.lo == lhs.hi && rhs.lo == rhs.hi && lhs.lo == rhs.lo;
    match op {
        CmpOp::Eq => {
            if single_equal {
                Some(true)
            } else if disjoint {
                Some(false)
            } else {
                None
            }
        }
        CmpOp::Ne => {
            if disjoint {
                Some(true)
            } else if single_equal {
                Some(false)
            } else {
                None
            }
        }
        CmpOp::Ge => {
            if lhs.lo >= rhs.hi {
                Some(true)
            } else if lhs.hi < rhs.lo {
                Some(false)
            } else {
                None
            }
        }
        CmpOp::Gt => {
            if lhs.lo > rhs.hi {
                Some(true)
            } else if lhs.hi <= rhs.lo {
                Some(false)
            } else {
                None
            }
        }
        CmpOp::Le => {
            if lhs.hi <= rhs.lo {
                Some(true)
            } else if lhs.lo > rhs.hi {
                Some(false)
            } else {
                None
            }
        }
        CmpOp::Lt => {
            if lhs.hi < rhs.lo {
                Some(true)
            } else if lhs.lo >= rhs.hi {
                Some(false)
            } else {
                None
            }
        }
    }
}

/// Upper bound on the id span a single Control Switches/Variables (121/122)
/// command may expand into. Real RPG Maker projects never address more than a
/// few thousand ids in one command; a malformed `parameters` range from a
/// corrupt or third-party file (e.g. `[1, 4_000_000_000]`) would otherwise make
/// the loop iterate billions of times and exhaust memory. Clamping the span
/// keeps garbage input bounded instead of hanging the analyzer.
pub(crate) const MAX_SYMBOL_RANGE: u64 = 100_000;

/// Normalizes a raw `(start, end)` id pair into an ascending, span-capped range.
/// Caps the width at [`MAX_SYMBOL_RANGE`] so a malformed range cannot blow up.
fn clamp_id_range(start: u64, end: u64) -> std::ops::RangeInclusive<u64> {
    let (lo, hi) = (start.min(end), start.max(end));
    lo..=hi.min(lo.saturating_add(MAX_SYMBOL_RANGE))
}

/// Ids of variables a command writes with a value not tracked by constant
/// propagation (122 over its whole range — even for a const-set, since only the
/// fact of the write matters when invalidating a loop body; 103/104/285 are
/// input/choice/map-data). Used to clear constants on loop entry.
fn command_var_writes(cmd: &EventCommand) -> Vec<u32> {
    match cmd.code {
        codes::CONTROL_VARIABLES => match (cmd.as_u64(0), cmd.as_u64(1)) {
            (Some(a), Some(b)) => clamp_id_range(a, b).map(|x| x as u32).collect(),
            _ => Vec::new(),
        },
        codes::INPUT_NUMBER | codes::SELECT_ITEM | codes::GET_LOCATION_INFO => {
            cmd.as_u64(0).map(|v| v as u32).into_iter().collect()
        }
        _ => Vec::new(),
    }
}

/// Invalidates in the environment the variables modified in a loop body (112 at `loop_idx`).
///
/// The body is the commands with an indent strictly greater than the loop's indent. If the body
/// contains an opaque command (script/common event/plugin command), we clear the entire
/// environment (any variable could have changed on the back edge); otherwise we invalidate
/// only the variables the body writes.
fn invalidate_loop_body_writes(env: &mut ConstEnv, list: &[EventCommand], loop_idx: usize) {
    let loop_indent = list[loop_idx].indent;
    let mut writes = Vec::new();
    let mut j = loop_idx + 1;
    while j < list.len() && list[j].indent > loop_indent {
        let c = &list[j];
        if matches!(
            c.code,
            codes::SCRIPT
                | codes::COMMON_EVENT
                | codes::PLUGIN_COMMAND_MV
                | codes::PLUGIN_COMMAND_MZ
        ) {
            env.clear();
            return;
        }
        writes.extend(command_var_writes(c));
        j += 1;
    }
    for v in writes {
        env.invalidate(v);
    }
}

/// Maps the numeric `comparison` code of a 111 condition (type 1) to [`CmpOp`].
fn cmp_op(code: u64) -> Option<CmpOp> {
    Some(match code {
        0 => CmpOp::Eq,
        1 => CmpOp::Ge,
        2 => CmpOp::Le,
        3 => CmpOp::Gt,
        4 => CmpOp::Lt,
        5 => CmpOp::Ne,
        _ => return None,
    })
}

/// Scope of a self-switch: the map+event within which the
/// command list executes (only for map event pages).
#[derive(Copy, Clone, Debug)]
pub struct SelfSwitchScope {
    /// Id of the event's map.
    pub map_id: u32,
    /// Id of the event on the map.
    pub event_id: u32,
}

/// Walk context for a single command list: the owner entity and the base path.
pub struct WalkCtx {
    /// The entity that owns the list (page / common event / troop page).
    pub entity: EntityId,
    /// The file the list lives in (`data/MapXXX.json` etc.).
    pub file: camino::Utf8PathBuf,
    /// Path segments down to the command list (without the segment of the command itself).
    pub base_path: Vec<PathSeg>,
    /// The self-switch scope (`Some` only for map event pages;
    /// `None` for common events and troop pages — there are no self-switches there).
    pub self_switch_scope: Option<SelfSwitchScope>,
    /// Global switches that must be ON for this command list to run (the page's
    /// switch conditions / a triggered common event's switch). Attached to every
    /// `121` ON write as its "gate" for the `circular-gate` rule. Empty means the
    /// list is not switch-gated (any switch it sets is freely settable).
    pub gate_switches: Vec<u32>,
}

impl WalkCtx {
    /// [`Location`] for the command at index `index` in the list.
    fn loc(&self, index: u32) -> Location {
        let mut segs = self.base_path.clone();
        segs.push(PathSeg::Command(index));
        Location::new(self.file.clone(), segs)
    }

    fn site(&self, index: u32) -> Site {
        Site {
            location: self.loc(index),
            entity: self.entity,
        }
    }
}

/// Walks the command list, filling `b` with edges and symbol sites.
///
/// Command 355 (Script) is handled with look-ahead: the body is assembled from
/// 355 + consecutive 655 into a single block and parsed by Tier B (see
/// [`script_block`]). Other commands go through [`interpret`].
pub fn walk(b: &mut IrBuilder, ctx: &WalkCtx, list: &[EventCommand]) {
    let mut env = ConstEnv::default();
    let mut i = 0usize;
    while i < list.len() {
        let cmd = &list[i];
        // Dominance: drop constants from closed blocks before processing.
        env.prune(cmd.indent);
        // Loop (112): because of the back edge, a variable modified anywhere in the body
        // has, on iterations 2+, a different value than on the forward pass. Before
        // walking the body we invalidate such variables (or the whole environment, if the body
        // contains an opaque command) — otherwise the condition/transfer would resolve against
        // a stale pre-loop constant (false dead branch).
        if cmd.code == codes::LOOP {
            invalidate_loop_body_writes(&mut env, list, i);
        }
        if cmd.code == codes::SCRIPT {
            // Full script block: 355 + the 655s that follow (continuations).
            let mut source = cmd.as_str(0).unwrap_or_default().to_string();
            let mut j = i + 1;
            while j < list.len() && list[j].code == codes::SCRIPT_CONT {
                source.push('\n');
                source.push_str(list[j].as_str(0).unwrap_or_default());
                j += 1;
            }
            script_block(b, ctx, &source, i as u32);
            // A script is opaque — it could have changed any variables.
            env.clear();
            i = j;
            continue;
        }
        interpret(b, ctx, cmd, i as u32, &mut env);
        // A common event call / plugin command may change variables outside our
        // visibility → we reset the environment after them (conservatively).
        if matches!(
            cmd.code,
            codes::COMMON_EVENT | codes::PLUGIN_COMMAND_MV | codes::PLUGIN_COMMAND_MZ
        ) {
            env.clear();
        }
        i += 1;
    }
}

fn interpret(b: &mut IrBuilder, ctx: &WalkCtx, cmd: &EventCommand, idx: u32, env: &mut ConstEnv) {
    match cmd.code {
        codes::SHOW_TEXT => {
            face_ref(b, ctx, cmd.as_str(0), idx);
            // MZ speaker name (parameters[4]) may embed a \v[n] escape.
            message_text_reads(b, ctx, cmd.as_str(4), idx);
        }
        // 401/405 — message/scrolling-text body lines: a \v[n] escape prints a
        // variable's value, which is a READ of that variable.
        codes::TEXT_DATA | codes::SCROLL_TEXT_DATA => {
            message_text_reads(b, ctx, cmd.as_str(0), idx)
        }
        // 102 — choice labels ([0] = array of strings) may embed \v[n].
        codes::SHOW_CHOICES => choice_text_reads(b, ctx, cmd, idx),
        codes::CONDITIONAL_BRANCH => conditional_branch(b, ctx, cmd, idx, env),

        codes::COMMON_EVENT => {
            if let Some(id) = cmd.as_u64(0) {
                let id = id as u32;
                if id != 0 {
                    edge(
                        b,
                        ctx,
                        Edge::CallsCommonEvent {
                            common_event_id: id,
                        },
                        idx,
                    );
                }
            }
        }

        codes::CONTROL_SWITCHES => control_switches(b, ctx, cmd, idx),
        codes::CONTROL_VARIABLES => control_variables(b, ctx, cmd, idx, env),
        // 123 — self-switch: a separate namespace. [0]=ch ("A".."D").
        // WRITE to the current event's self-switch (only if a scope exists).
        codes::CONTROL_SELF_SWITCH => {
            if let Some(ch) = cmd.as_str(0).and_then(|s| s.chars().next()) {
                self_switch_write(b, ctx, ch, idx);
            }
        }

        // 103/104/285 write a variable with a value unknown to static analysis (player
        // input / choice / map data) → invalidate its constant in the environment.
        codes::INPUT_NUMBER => {
            if let Some(v) = cmd.as_u64(0) {
                write_var(b, ctx, v as u32, idx);
                env.invalidate(v as u32);
            }
        }
        codes::SELECT_ITEM => {
            if let Some(v) = cmd.as_u64(0) {
                write_var(b, ctx, v as u32, idx);
                env.invalidate(v as u32);
            }
        }
        codes::GET_LOCATION_INFO => {
            if let Some(v) = cmd.as_u64(0) {
                write_var(b, ctx, v as u32, idx);
                env.invalidate(v as u32);
            }
            // [2]=designation; by variable (1) → [3]/[4] are x/y variableIds READ.
            if cmd.as_u64(2) == Some(1) {
                read_var_slot(b, ctx, cmd, 3, idx);
                read_var_slot(b, ctx, cmd, 4, idx);
            }
        }

        // 125/126/127/128 — Gold/Items/Weapons/Armors: the amount may be given by
        // a variable (operandType==1 → the value slot is a variableId READ).
        codes::CHANGE_GOLD => operand_var_read(b, ctx, cmd, 1, 2, idx),
        codes::CHANGE_ITEMS => {
            db_ref0(b, ctx, DbKind::Item, cmd, idx);
            operand_var_read(b, ctx, cmd, 2, 3, idx);
        }
        codes::CHANGE_WEAPONS => {
            db_ref0(b, ctx, DbKind::Weapon, cmd, idx);
            operand_var_read(b, ctx, cmd, 2, 3, idx);
        }
        codes::CHANGE_ARMORS => {
            db_ref0(b, ctx, DbKind::Armor, cmd, idx);
            operand_var_read(b, ctx, cmd, 2, 3, idx);
        }
        codes::CHANGE_PARTY_MEMBER => db_ref0(b, ctx, DbKind::Actor, cmd, idx),
        codes::NAME_INPUT => db_ref0(b, ctx, DbKind::Actor, cmd, idx),

        codes::TRANSFER_PLAYER => transfer(b, ctx, cmd, idx, env),
        // 202 Set Vehicle Location: [1]=designation; by variable (1) → [2]/[3]/[4]
        // are map/x/y variableIds READ.
        codes::SET_VEHICLE_LOCATION => {
            if cmd.as_u64(1) == Some(1) {
                read_var_slot(b, ctx, cmd, 2, idx);
                read_var_slot(b, ctx, cmd, 3, idx);
                read_var_slot(b, ctx, cmd, 4, idx);
            }
        }
        // 203 Set Event Location: [1]=type(0 direct/1 variable/2 swap); by
        // variable (1) → [2]/[3] are x/y variableIds READ.
        codes::SET_EVENT_LOCATION => {
            if cmd.as_u64(1) == Some(1) {
                read_var_slot(b, ctx, cmd, 2, idx);
                read_var_slot(b, ctx, cmd, 3, idx);
            }
        }
        // 212 Show Animation / 337 Show Battle Animation: [1]=animationId DB ref.
        codes::SHOW_ANIMATION | codes::SHOW_BATTLE_ANIMATION => {
            if let Some(id) = cmd.as_u64(1)
                && id != 0
            {
                db_ref(b, ctx, DbKind::Animation, id as u32, idx);
            }
        }
        codes::BATTLE_PROCESSING => battle(b, ctx, cmd, idx, env),
        codes::SHOP_PROCESSING | codes::SHOP_GOODS_ROW => shop_row(b, ctx, cmd, idx),

        // actorEx target commands (311–318/326).
        c if codes::ACTOR_EX_TARGET.contains(&c) => {
            actor_ex_target(b, ctx, cmd, idx);
            match c {
                codes::CHANGE_STATE => actor_ex_extra_db(b, ctx, DbKind::State, cmd, 3, idx),
                codes::CHANGE_SKILL => actor_ex_extra_db(b, ctx, DbKind::Skill, cmd, 3, idx),
                // Stat delta by variable: operandType==1 → the value slot is a
                // variableId READ. HP/MP/TP/EXP/Level keep operandType at [3] and
                // the value at [4]; Change Parameter (317) shifts them to [4]/[5]
                // because slot [2] holds the paramId.
                codes::CHANGE_HP
                | codes::CHANGE_MP
                | codes::CHANGE_TP
                | codes::CHANGE_EXP
                | codes::CHANGE_LEVEL => operand_var_read(b, ctx, cmd, 3, 4, idx),
                codes::CHANGE_PARAMETER => operand_var_read(b, ctx, cmd, 4, 5, idx),
                // 314 Recover All: no operand.
                _ => {}
            }
        }

        codes::CHANGE_EQUIPMENT => change_equipment(b, ctx, cmd, idx),
        codes::CHANGE_NAME => db_ref0(b, ctx, DbKind::Actor, cmd, idx),
        codes::CHANGE_NICKNAME => db_ref0(b, ctx, DbKind::Actor, cmd, idx),
        codes::CHANGE_PROFILE => db_ref0(b, ctx, DbKind::Actor, cmd, idx),
        codes::CHANGE_CLASS => {
            db_ref0(b, ctx, DbKind::Actor, cmd, idx);
            if let Some(id) = cmd.as_u64(1)
                && id != 0
            {
                db_ref(b, ctx, DbKind::Class, id as u32, idx);
            }
        }
        codes::CHANGE_ACTOR_IMAGES => change_actor_images(b, ctx, cmd, idx),
        codes::CHANGE_VEHICLE_IMAGE => {
            asset_ref(b, ctx, AssetKind::Character, cmd.as_str(1), idx);
        }
        // 331/332/342 Change Enemy HP/MP/TP (troop battle events): target is a
        // plain enemy index ([0]), but the delta may be by variable —
        // operateValue([1]=operation, [2]=operandType, [3]=operand): [2]==1 →
        // [3] is a variableId READ.
        codes::CHANGE_ENEMY_HP | codes::CHANGE_ENEMY_MP | codes::CHANGE_ENEMY_TP => {
            operand_var_read(b, ctx, cmd, 2, 3, idx)
        }
        codes::CHANGE_ENEMY_STATE => {
            // [0]=index of the troop member (not actorId), [2]=stateId.
            if let Some(id) = cmd.as_u64(2)
                && id != 0
            {
                db_ref(b, ctx, DbKind::State, id as u32, idx);
            }
        }
        codes::ENEMY_TRANSFORM => {
            if let Some(id) = cmd.as_u64(1)
                && id != 0
            {
                db_ref(b, ctx, DbKind::Enemy, id as u32, idx);
            }
        }

        codes::SHOW_PICTURE => {
            asset_ref(b, ctx, AssetKind::Picture, cmd.as_str(1), idx);
            picture_position_var_reads(b, ctx, cmd, idx);
        }
        // 232 Move Picture: no asset name, but the same position-by-variable slots.
        codes::MOVE_PICTURE => picture_position_var_reads(b, ctx, cmd, idx),
        codes::PLAY_BGM => asset_ref(b, ctx, AssetKind::Bgm, cmd.audio_name(0), idx),
        codes::PLAY_BGS => asset_ref(b, ctx, AssetKind::Bgs, cmd.audio_name(0), idx),
        codes::PLAY_ME => asset_ref(b, ctx, AssetKind::Me, cmd.audio_name(0), idx),
        codes::PLAY_SE => asset_ref(b, ctx, AssetKind::Se, cmd.audio_name(0), idx),
        codes::PLAY_MOVIE => asset_ref(b, ctx, AssetKind::Movie, cmd.as_str(0), idx),
        codes::CHANGE_TILESET => {
            // [0]=tilesetId — an indirect reference to a Tilesets entry (names are resolved by the assets rule).
            if let Some(id) = cmd.as_u64(0)
                && id != 0
            {
                db_ref(b, ctx, DbKind::Tileset, id as u32, idx);
            }
        }
        codes::CHANGE_BATTLE_BACK => {
            asset_ref(b, ctx, AssetKind::Battleback1, cmd.as_str(0), idx);
            asset_ref(b, ctx, AssetKind::Battleback2, cmd.as_str(1), idx);
        }
        codes::CHANGE_PARALLAX => asset_ref(b, ctx, AssetKind::Parallax, cmd.as_str(0), idx),

        // 355/655 (Script) are handled as a block in `walk` (look-ahead).

        // 357 (MZ): structured plugin command call [0]=plugin, [1]=command.
        codes::PLUGIN_COMMAND_MZ => plugin_command_mz(b, ctx, cmd, idx),
        // 356 (MV): raw string "PluginName arg1 arg2"; the command is the first token.
        codes::PLUGIN_COMMAND_MV => plugin_command_mv(b, ctx, cmd, idx),

        _ => {}
    }
}

// --- Edge/site helpers ---

fn edge(b: &mut IrBuilder, ctx: &WalkCtx, e: Edge, idx: u32) {
    b.push_edge(ctx.entity, e, ctx.loc(idx));
}

fn db_ref(b: &mut IrBuilder, ctx: &WalkCtx, kind: DbKind, id: u32, idx: u32) {
    edge(b, ctx, Edge::ReferencesDbId { kind, id }, idx);
}

/// Reference to a DB entry from `parameters[0]` (0 = "none", skipped).
fn db_ref0(b: &mut IrBuilder, ctx: &WalkCtx, kind: DbKind, cmd: &EventCommand, idx: u32) {
    if let Some(id) = cmd.as_u64(0)
        && id != 0
    {
        db_ref(b, ctx, kind, id as u32, idx);
    }
}

fn read_switch(b: &mut IrBuilder, ctx: &WalkCtx, id: u32, idx: u32) {
    if id == 0 {
        return;
    }
    b.symbols_mut().add_switch_read(id, ctx.site(idx));
    edge(b, ctx, Edge::ReadsSwitch { switch_id: id }, idx);
}

fn write_switch(b: &mut IrBuilder, ctx: &WalkCtx, id: u32, idx: u32) {
    if id == 0 {
        return;
    }
    b.symbols_mut().add_switch_write(id, ctx.site(idx));
    edge(b, ctx, Edge::WritesSwitch { switch_id: id }, idx);
}

fn read_var(b: &mut IrBuilder, ctx: &WalkCtx, id: u32, idx: u32) {
    if id == 0 {
        return;
    }
    b.symbols_mut().add_variable_read(id, ctx.site(idx));
    edge(b, ctx, Edge::ReadsVariable { variable_id: id }, idx);
}

fn write_var(b: &mut IrBuilder, ctx: &WalkCtx, id: u32, idx: u32) {
    if id == 0 {
        return;
    }
    b.symbols_mut().add_variable_write(id, ctx.site(idx));
    edge(b, ctx, Edge::WritesVariable { variable_id: id }, idx);
}

/// Reads the variable whose id sits in `parameters[slot]` (skipped if absent).
/// For "x/y/map by variable" slot groups (201/202/203/231/285).
fn read_var_slot(b: &mut IrBuilder, ctx: &WalkCtx, cmd: &EventCommand, slot: usize, idx: u32) {
    if let Some(v) = cmd.as_u64(slot) {
        read_var(b, ctx, v as u32, idx);
    }
}

/// Registers a variable READ for the shared "operand by variable" convention: when
/// the operand-type slot at `type_idx` is `1` (variable), the slot at `value_idx`
/// holds a variableId. Covers Change HP/MP/TP/EXP/Level/Parameter and
/// Change Gold/Items/Weapons/Armors (§1.4). Operand type `0` is a literal amount.
fn operand_var_read(
    b: &mut IrBuilder,
    ctx: &WalkCtx,
    cmd: &EventCommand,
    type_idx: usize,
    value_idx: usize,
    idx: u32,
) {
    if cmd.as_u64(type_idx) == Some(1) {
        read_var_slot(b, ctx, cmd, value_idx, idx);
    }
}

/// Show/Move Picture (231/232) position designation: `[3]==1` (by variable) →
/// `[4]`/`[5]` are x/y variableIds READ.
fn picture_position_var_reads(b: &mut IrBuilder, ctx: &WalkCtx, cmd: &EventCommand, idx: u32) {
    if cmd.as_u64(3) == Some(1) {
        read_var_slot(b, ctx, cmd, 4, idx);
        read_var_slot(b, ctx, cmd, 5, idx);
    }
}

/// Collects variable ids read via `\v[n]` / `\V[n]` escapes in message text,
/// preserving first-seen order and de-duplicating. Mirrors RPG Maker's
/// `convertEscapeCharacters` (`\\V\[(\d+)\]`, case-insensitive): only a literal
/// digit index is a statically-known read. A nested form `\v[\v[3]]` contributes
/// the inner literal (3) — the outer index is dynamic and skipped, exactly the
/// order the engine resolves them. Id `0` (placeholder) is ignored.
pub(crate) fn collect_text_var_ids(text: &str, out: &mut Vec<u32>) {
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\\'
            && matches!(bytes.get(i + 1), Some(b'v' | b'V'))
            && bytes.get(i + 2) == Some(&b'[')
        {
            let start = i + 3;
            let mut k = start;
            while k < bytes.len() && bytes[k].is_ascii_digit() {
                k += 1;
            }
            if k > start && bytes.get(k) == Some(&b']') {
                if let Ok(n) = text[start..k].parse::<u32>()
                    && n != 0
                    && !out.contains(&n)
                {
                    out.push(n);
                }
                i = k + 1;
                continue;
            }
        }
        i += 1;
    }
}

/// Registers a variable READ for every `\v[n]` escape in a single text string.
fn message_text_reads(b: &mut IrBuilder, ctx: &WalkCtx, text: Option<&str>, idx: u32) {
    let Some(text) = text else { return };
    let mut ids = Vec::new();
    collect_text_var_ids(text, &mut ids);
    for id in ids {
        read_var(b, ctx, id, idx);
    }
}

/// Registers variable reads for `\v[n]` escapes across all choice labels of a
/// `102` Show Choices command (`parameters[0]` = array of strings).
fn choice_text_reads(b: &mut IrBuilder, ctx: &WalkCtx, cmd: &EventCommand, idx: u32) {
    let mut ids = Vec::new();
    for s in cmd.as_str_array(0) {
        collect_text_var_ids(s, &mut ids);
    }
    for id in ids {
        read_var(b, ctx, id, idx);
    }
}

fn asset_ref(b: &mut IrBuilder, ctx: &WalkCtx, kind: AssetKind, name: Option<&str>, idx: u32) {
    let Some(name) = name else { return };
    if name.is_empty() {
        return;
    }
    // Strip the plugin tag prefix `[…]` so the reference matches the normalized
    // file name on disk (see `assets::strip_bracket_prefix`).
    let name = crate::assets::strip_bracket_prefix(name);
    if name.is_empty() {
        return;
    }
    let key = AssetKey::new(kind, name);
    let loc = ctx.loc(idx);
    b.add_asset_ref(key.clone(), loc.clone());
    edge(b, ctx, Edge::ReferencesAsset { asset: key }, idx);
}

fn face_ref(b: &mut IrBuilder, ctx: &WalkCtx, name: Option<&str>, idx: u32) {
    asset_ref(b, ctx, AssetKind::Face, name, idx);
}

/// Self-switch key for the current scope+ch (if a scope is set and the channel is valid).
fn self_switch_key(ctx: &WalkCtx, ch: char) -> Option<SelfSwitchKey> {
    let scope = ctx.self_switch_scope?;
    if scope.event_id == 0 {
        return None;
    }
    Some(SelfSwitchKey::new(scope.map_id, scope.event_id, ch))
}

/// Writes the current event's self-switch (command 123).
fn self_switch_write(b: &mut IrBuilder, ctx: &WalkCtx, ch: char, idx: u32) {
    if let Some(key) = self_switch_key(ctx, ch) {
        b.add_self_switch_write(key, ctx.site(idx));
    }
}

/// Reads the current event's self-switch (111 type 2 / page condition).
fn self_switch_read(b: &mut IrBuilder, ctx: &WalkCtx, ch: char, idx: u32) {
    if let Some(key) = self_switch_key(ctx, ch) {
        b.add_self_switch_read(key, ctx.site(idx));
    }
}

// --- Commands with nontrivial logic ---

fn conditional_branch(
    b: &mut IrBuilder,
    ctx: &WalkCtx,
    cmd: &EventCommand,
    idx: u32,
    env: &mut ConstEnv,
) {
    let Some(ty) = cmd.as_u64(0) else { return };
    match ty {
        0 => {
            // Switch: [1]=switchId READ.
            if let Some(id) = cmd.as_u64(1) {
                read_switch(b, ctx, id as u32, idx);
            }
        }
        1 => {
            // Variable: [1]=varId READ; [3]=srcVar READ if [2]==1; [4]=operator.
            if let Some(id) = cmd.as_u64(1) {
                read_var(b, ctx, id as u32, idx);
            }
            if cmd.as_u64(2) == Some(1)
                && let Some(id) = cmd.as_u64(3)
            {
                read_var(b, ctx, id as u32, idx);
            }
            condition_dead_branch(b, ctx, cmd, idx, env);
        }
        2 => {
            // Self-switch READ: [1]=ch ("A".."D"). A separate namespace.
            if let Some(ch) = cmd.as_str(1).and_then(|s| s.chars().next()) {
                self_switch_read(b, ctx, ch, idx);
            }
        }
        4 => {
            // Actor: [1]=actorId, [3]=operand (class/skill/weapon/armor/state per [2]).
            if let Some(id) = cmd.as_u64(1)
                && id != 0
            {
                db_ref(b, ctx, DbKind::Actor, id as u32, idx);
            }
            if let (Some(check), Some(operand)) = (cmd.as_u64(2), cmd.as_u64(3)) {
                let kind = match check {
                    2 => Some(DbKind::Class),
                    3 => Some(DbKind::Skill),
                    4 => Some(DbKind::Weapon),
                    5 => Some(DbKind::Armor),
                    6 => Some(DbKind::State),
                    _ => None,
                };
                if let Some(kind) = kind
                    && operand != 0
                {
                    db_ref(b, ctx, kind, operand as u32, idx);
                }
            }
        }
        5 => {
            // Enemy: [3]=stateId if [2]==1.
            if cmd.as_u64(2) == Some(1)
                && let Some(id) = cmd.as_u64(3)
                && id != 0
            {
                db_ref(b, ctx, DbKind::State, id as u32, idx);
            }
        }
        8 => db_ref1(b, ctx, DbKind::Item, cmd, idx),
        9 => db_ref1(b, ctx, DbKind::Weapon, cmd, idx),
        10 => db_ref1(b, ctx, DbKind::Armor, cmd, idx),
        // 12 — script condition: [1]=raw JS. Blackbox + Tier B extraction of writes.
        // The expression is evaluated by the engine as arbitrary JavaScript, so it
        // may mutate variables as a side effect; clear constant propagation just
        // like for an explicit 355 script block.
        12 => {
            if let Some(s) = cmd.as_str(1) {
                script_block(b, ctx, s, idx);
                env.clear();
            }
        }
        _ => {}
    }
}

/// Attempts to resolve a 111 condition (type 1, variable comparison) against the
/// current symbolic ranges. If the known range of the left variable and of the
/// right operand make the comparison always true or always false, registers a
/// dead branch.
///
/// 111 parameters (type 1): `[1]`=varId, `[2]`=operand type (0 const / 1 variable),
/// `[3]`=value/srcVarId, `[4]`=comparison operator code.
fn condition_dead_branch(
    b: &mut IrBuilder,
    ctx: &WalkCtx,
    cmd: &EventCommand,
    idx: u32,
    env: &ConstEnv,
) {
    let Some(var_id) = cmd.as_u64(1) else { return };
    let var_id = var_id as u32;
    let Some(lhs) = env.get(var_id) else { return };
    let Some(op) = cmd.as_u64(4).and_then(cmp_op) else {
        return;
    };
    let rhs = match cmd.as_u64(2) {
        Some(0) => cmd.as_i64(3).map(Interval::exact),
        Some(1) => cmd.as_u64(3).and_then(|src| env.get(src as u32)),
        _ => None,
    };
    let Some(rhs) = rhs else { return };
    let Some(result) = interval_cmp_definite(lhs, op, rhs) else {
        return;
    };
    b.add_dead_branch(DeadBranch {
        location: ctx.loc(idx),
        var_id,
        value_lo: lhs.lo,
        value_hi: lhs.hi,
        op,
        operand_lo: rhs.lo,
        operand_hi: rhs.hi,
        result,
    });
}

/// Reference to a DB entry from `parameters[1]` (for 111 type 8/9/10).
fn db_ref1(b: &mut IrBuilder, ctx: &WalkCtx, kind: DbKind, cmd: &EventCommand, idx: u32) {
    if let Some(id) = cmd.as_u64(1)
        && id != 0
    {
        db_ref(b, ctx, kind, id as u32, idx);
    }
}

fn control_switches(b: &mut IrBuilder, ctx: &WalkCtx, cmd: &EventCommand, idx: u32) {
    let (Some(start), Some(end)) = (cmd.as_u64(0), cmd.as_u64(1)) else {
        return;
    };
    // [2] = value (0 ON / 1 OFF). An OFF write makes a gating switch "clearable".
    let set_off = cmd.as_u64(2) == Some(1);
    for id in clamp_id_range(start, end) {
        write_switch(b, ctx, id as u32, idx);
        if id != 0 {
            if set_off {
                b.symbols_mut().mark_switch_ever_set_off(id as u32);
            } else {
                // An ON write behind the enclosing list's gate — input to
                // `circular-gate` (progression-deadlock detection).
                b.add_switch_gate(SwitchGate {
                    switch_id: id as u32,
                    gate: ctx.gate_switches.clone(),
                    location: ctx.loc(idx),
                });
            }
        }
    }
}

fn control_variables(
    b: &mut IrBuilder,
    ctx: &WalkCtx,
    cmd: &EventCommand,
    idx: u32,
    env: &mut ConstEnv,
) {
    let (Some(start), Some(end)) = (cmd.as_u64(0), cmd.as_u64(1)) else {
        return;
    };
    // Span-capped to keep a malformed range from hanging the analyzer.
    let range = clamp_id_range(start, end);
    let (lo, hi) = (*range.start(), *range.end());
    for id in lo..=hi {
        write_var(b, ctx, id as u32, idx);
    }
    // Operand type 1 (variable) → [4] is srcVarId READ.
    if cmd.as_u64(3) == Some(1)
        && let Some(src) = cmd.as_u64(4)
    {
        read_var(b, ctx, src as u32, idx);
    }
    // Operand type 4 (script) → [4] is raw JS. Blackbox + Tier B extraction.
    if cmd.as_u64(3) == Some(4)
        && let Some(s) = cmd.as_str(4)
    {
        script_block(b, ctx, s, idx);
        // The script operand is evaluated as arbitrary JavaScript. Besides
        // producing the target value, it may call $gameVariables.setValue or
        // otherwise mutate game state, so any previously propagated constants are
        // no longer trustworthy.
        env.clear();
    }

    // Update symbolic-range propagation. We support set / add / sub with a
    // literal, a known source variable, or a random min..max range; everything
    // else (mul/div/mod, game data, script) makes the variable unknown
    // (conservatively: fewer findings, but not false ones).
    // [2]=operation (0 set,1 add,2 sub), [3]=operand type
    // (0 const,1 variable,2 random), [4]=operand ([5]=max for random).
    let operand_iv: Option<Interval> = match cmd.as_u64(3) {
        Some(0) | None => cmd.as_i64(4).map(Interval::exact),
        Some(1) => cmd.as_u64(4).and_then(|src| env.get(src as u32)),
        Some(2) => match (cmd.as_i64(4), cmd.as_i64(5)) {
            (Some(a), Some(b)) => Some(Interval::range(a, b)),
            _ => None,
        },
        _ => None,
    };
    let operation = cmd.as_u64(2).unwrap_or(0);
    for id in lo..=hi {
        // add/sub need the target's current range; a missing one → unknown.
        let new_iv = match operation {
            0 => operand_iv,
            1 => match (env.get(id as u32), operand_iv) {
                (Some(cur), Some(op)) => Some(cur.add(op)),
                _ => None,
            },
            2 => match (env.get(id as u32), operand_iv) {
                (Some(cur), Some(op)) => Some(cur.sub(op)),
                _ => None,
            },
            _ => None,
        };
        match new_iv {
            Some(v) => env.set(id as u32, v, cmd.indent),
            None => env.invalidate(id as u32),
        }
    }
}

fn transfer(b: &mut IrBuilder, ctx: &WalkCtx, cmd: &EventCommand, idx: u32, env: &ConstEnv) {
    match cmd.as_u64(0) {
        Some(0) => {
            // Direct designation: [1]=mapId.
            let to_map = cmd.as_u64(1).map(|m| m as u32);
            edge(
                b,
                ctx,
                Edge::Transfer {
                    to_map,
                    designation: TransferDesignation::Direct,
                },
                idx,
            );
        }
        Some(1) => {
            // By variable: [1]/[2]/[3] are varId READ; the map is not static —
            // but if [1] is constant-resolvable (constant propagation), we take it
            // as the target map (`broken-transfer` will check it with likely confidence).
            for i in 1..=3 {
                if let Some(v) = cmd.as_u64(i) {
                    read_var(b, ctx, v as u32, idx);
                }
            }
            let to_map = cmd
                .as_u64(1)
                .and_then(|v| env.get_exact(v as u32))
                .filter(|&m| m > 0 && m <= u32::MAX as i64)
                .map(|m| m as u32);
            edge(
                b,
                ctx,
                Edge::Transfer {
                    to_map,
                    designation: TransferDesignation::ByVariable,
                },
                idx,
            );
        }
        _ => {}
    }
}

fn battle(b: &mut IrBuilder, ctx: &WalkCtx, cmd: &EventCommand, idx: u32, env: &ConstEnv) {
    match cmd.as_u64(0) {
        Some(0) => {
            // Direct: [1]=troopId.
            if let Some(id) = cmd.as_u64(1)
                && id != 0
            {
                db_ref(b, ctx, DbKind::Troop, id as u32, idx);
            }
        }
        Some(1) => {
            // By variable: [1]=varId READ. If the value is constant-resolvable —
            // we emit a troop reference (`referential-integrity` will check it).
            if let Some(v) = cmd.as_u64(1) {
                read_var(b, ctx, v as u32, idx);
                if let Some(troop) = env
                    .get_exact(v as u32)
                    .filter(|&t| t > 0 && t <= u32::MAX as i64)
                {
                    db_ref(b, ctx, DbKind::Troop, troop as u32, idx);
                }
            }
        }
        _ => {}
    }
}

fn shop_row(b: &mut IrBuilder, ctx: &WalkCtx, cmd: &EventCommand, idx: u32) {
    // [0]=type(0 item/1 weapon/2 armor), [1]=dataId.
    let (Some(ty), Some(id)) = (cmd.as_u64(0), cmd.as_u64(1)) else {
        return;
    };
    if id == 0 {
        return;
    }
    let kind = match ty {
        0 => DbKind::Item,
        1 => DbKind::Weapon,
        2 => DbKind::Armor,
        _ => return,
    };
    db_ref(b, ctx, kind, id as u32, idx);
}

/// `iterateActorEx([0],[1])` convention: [0]==0 → literal actorId,
/// [0]==1 → variableId (READ).
fn actor_ex_target(b: &mut IrBuilder, ctx: &WalkCtx, cmd: &EventCommand, idx: u32) {
    match cmd.as_u64(0) {
        Some(0) => {
            // [1]=actorId; 0 = the whole party (not a dangling reference).
            if let Some(id) = cmd.as_u64(1)
                && id != 0
            {
                db_ref(b, ctx, DbKind::Actor, id as u32, idx);
            }
        }
        Some(1) => {
            if let Some(v) = cmd.as_u64(1) {
                read_var(b, ctx, v as u32, idx);
            }
        }
        _ => {}
    }
}

/// Additional DB reference from a fixed index for actorEx commands (313/318).
fn actor_ex_extra_db(
    b: &mut IrBuilder,
    ctx: &WalkCtx,
    kind: DbKind,
    cmd: &EventCommand,
    i: usize,
    idx: u32,
) {
    if let Some(id) = cmd.as_u64(i)
        && id != 0
    {
        db_ref(b, ctx, kind, id as u32, idx);
    }
}

fn change_equipment(b: &mut IrBuilder, ctx: &WalkCtx, cmd: &EventCommand, idx: u32) {
    // 319 Change Equipment: [0]=actorId, [1]=equip slot, [2]=itemId.
    // Slot 1 is the weapon slot in the default RPG Maker equip model; other
    // slots hold armor entries. Slot 0 is not a valid equip slot in event data.
    db_ref0(b, ctx, DbKind::Actor, cmd, idx);
    let (Some(slot), Some(id)) = (cmd.as_u64(1), cmd.as_u64(2)) else {
        return;
    };
    if id == 0 || slot == 0 {
        return;
    }
    let kind = if slot == 1 {
        DbKind::Weapon
    } else {
        DbKind::Armor
    };
    db_ref(b, ctx, kind, id as u32, idx);
}

fn change_actor_images(b: &mut IrBuilder, ctx: &WalkCtx, cmd: &EventCommand, idx: u32) {
    // [0]=actorId, [1]=characterName, [3]=faceName, [5]=battlerName(sv_actors).
    db_ref0(b, ctx, DbKind::Actor, cmd, idx);
    asset_ref(b, ctx, AssetKind::Character, cmd.as_str(1), idx);
    asset_ref(b, ctx, AssetKind::Face, cmd.as_str(3), idx);
    asset_ref(b, ctx, AssetKind::SvActor, cmd.as_str(5), idx);
}

/// Handles a script block (355+655 / 111-type-12 / 122-operand-4): Tier B
/// extracts literal writes to **global switches** (a write edge from the owner
/// entity — provides a `stuck-autorun` exit and quiets `uninitialized`) and
/// current-event **self-switch** reads/writes (bound to this event's scope), then
/// stores the opaque blackbox (its body is not analyzed further). An empty source
/// is skipped.
///
/// Writes to **variables** from scripts are deliberately NOT emitted as sites: on
/// the corpus they produced false `dead-variables` (a script writes a variable read
/// only in another script — we do not extract reads from scripts). For switches there
/// is no "dead write" rule, so they are safe. Self-switches are emitted only for the
/// CURRENT-EVENT idiom (`$gameSelfSwitches.setValue/value([this._mapId,
/// this._eventId, 'X'], …)`), which binds to a known `(map, event)`; a foreign or
/// computed key (`[this._mapId, 9, 'A']`) stays opaque to avoid cross-event
/// misattribution.
fn script_block(b: &mut IrBuilder, ctx: &WalkCtx, source: &str, idx: u32) {
    use dk_doctor_core::ir::{Entity, ScriptBlackbox};
    if source.is_empty() {
        return;
    }
    let facts = crate::plugins::js::analyze_script(source);
    for id in facts.switch_writes {
        write_switch(b, ctx, id, idx);
        // The script value is unknown (ON or OFF) — mark the switch as opaquely
        // written so `circular-gate` treats it as freely settable, not deadlocked.
        b.mark_switch_script_written(id);
    }
    // Current-event self-switch reads/writes ([this._mapId, this._eventId, 'X']):
    // bound to this event's scope — clears false unreachable/dead-self-switch when a
    // script (not command 123) toggles the event's own self-switch.
    for ch in facts.self_switch_writes {
        self_switch_write(b, ctx, ch, idx);
    }
    for ch in facts.self_switch_reads {
        self_switch_read(b, ctx, ch, idx);
    }
    // Literal variable reads ($gameVariables.value(N)) from the script: mark them
    // so dead-variables does not flag a variable consumed only here as dead.
    for id in facts.variable_reads {
        b.symbols_mut().mark_variable_read_by_plugin(id);
    }
    // A common event reserved by a script ($gameTemp.reserveCommonEvent(N)) — saves
    // it from `dead-common-event` (it runs deferred, not as a 117).
    for id in facts.reserved_common_events {
        b.add_reserved_common_event(id);
    }
    b.push_entity(
        Entity::Script(ScriptBlackbox {
            source: source.to_string(),
        }),
        ctx.loc(idx),
    );
}

/// 357 (MZ Plugin Command): `[0]`=plugin name, `[1]`=command name (structured).
fn plugin_command_mz(b: &mut IrBuilder, ctx: &WalkCtx, cmd: &EventCommand, idx: u32) {
    let plugin = cmd.as_str(0).unwrap_or_default().trim();
    let command = cmd.as_str(1).unwrap_or_default().trim();
    // Empty command — nothing to check (broken/service call).
    if command.is_empty() {
        return;
    }
    b.add_plugin_command_call(
        PluginCommandCall {
            plugin: (!plugin.is_empty()).then(|| plugin.to_string()),
            command: command.to_string(),
            structured: true,
        },
        ctx.loc(idx),
    );
}

/// 356 (MV Plugin Command): `[0]`=raw string `"PluginName arg1 arg2"`; the command
/// name is the first token (best-effort: MV does not separate plugin and command).
fn plugin_command_mv(b: &mut IrBuilder, ctx: &WalkCtx, cmd: &EventCommand, idx: u32) {
    let raw = cmd.as_str(0).unwrap_or_default().trim();
    let Some(command) = raw.split_whitespace().next() else {
        return;
    };
    if command.is_empty() {
        return;
    }
    b.add_plugin_command_call(
        PluginCommandCall {
            plugin: None,
            command: command.to_string(),
            structured: false,
        },
        ctx.loc(idx),
    );
}
