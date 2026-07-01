//! Spatial (tile passability) analysis: transfers / the player start landing on
//! a tile impassable from all four directions.
//!
//! RPG Maker passage specifics live here (the layered `data` array, the tileset
//! passage flags), so the core stays engine-independent. Faithful to
//! `Game_Map.checkPassage`: for a tile position the **topmost non-star** layer's
//! flag decides passability; when its four direction bits are all set (`0x0f`) the
//! player cannot leave in any direction (a soft-lock). Feeds the `blocked-tile`
//! rule via [`IrBuilder::add_blocked_tile`].
//!
//! Deliberately conservative (the rule is `likely`, off by default): a fact is
//! emitted only when a determining tile with all four impassable bits exists, and
//! never when the grid/flags are missing or coordinates are out of bounds.

use crate::codes;
use crate::command::EventCommand;
use crate::raw::map::Map;
use crate::raw::system::System;
use camino::Utf8PathBuf;
use dk_doctor_core::ir::{BlockedTile, BlockedTileKind, IrBuilder, Location, PathSeg};
use rustc_hash::FxHashMap;

/// Star bit (`☆`): the tile is passable and drawn above the character — it has no
/// effect on passage, so `checkPassage` skips it and looks at the layer below.
const FLAG_STAR: u32 = 0x10;
/// All four direction bits set (down/left/right/up) — impassable from every side.
const FLAG_ALL_DIRECTIONS: u32 = 0x0f;

/// Geometry + tiles of one map, retained for cross-map transfer resolution.
struct MapGrid {
    width: u32,
    height: u32,
    tileset_id: u32,
    /// Flat layered tile-id array (`(z*height + y)*width + x`).
    data: Vec<i32>,
}

/// A fixed Transfer Player (201, Direct) destination with its command location.
struct TransferDest {
    map_id: u32,
    x: u32,
    y: u32,
    location: Location,
}

/// Accumulates, across the whole project, what the spatial pass needs: every map's
/// grid (transfer targets can be any map) and every fixed transfer destination.
#[derive(Default)]
pub struct Collector {
    grids: FxHashMap<u32, MapGrid>,
    transfers: Vec<TransferDest>,
}

impl Collector {
    /// Records a map's grid and scans its event pages for fixed (Direct) transfers.
    ///
    /// Takes the parsed map by value to avoid cloning the (large) tile array — the
    /// caller no longer needs it after the IR walk. Common-event / troop-page
    /// transfers are intentionally out of scope (map navigation is the common
    /// case; MVP).
    pub fn add_map(&mut self, map_id: u32, file: &Utf8PathBuf, m: Map) {
        for event in m.events.iter().flatten() {
            if event.id == 0 {
                continue;
            }
            for (pi, page) in event.pages.iter().enumerate() {
                let page_no = (pi + 1) as u32;
                for (ci, cmd) in page.list.iter().enumerate() {
                    if let Some((tmap, x, y)) = direct_transfer(cmd) {
                        let location = Location::new(
                            file.clone(),
                            vec![
                                PathSeg::Map(map_id),
                                PathSeg::Event(event.id),
                                PathSeg::Page(page_no),
                                PathSeg::Command(ci as u32),
                            ],
                        );
                        self.transfers.push(TransferDest {
                            map_id: tmap,
                            x,
                            y,
                            location,
                        });
                    }
                }
            }
        }
        self.grids.insert(
            map_id,
            MapGrid {
                width: m.width,
                height: m.height,
                tileset_id: m.tileset_id,
                data: m.data,
            },
        );
    }
}

/// A Direct Transfer Player (`201`, designation `[0]==0`): `[1]`=mapId, `[2]`=x,
/// `[3]`=y. `None` for by-variable transfers (dynamic) or the unset map 0.
fn direct_transfer(cmd: &EventCommand) -> Option<(u32, u32, u32)> {
    if cmd.code != codes::TRANSFER_PLAYER || cmd.as_u64(0) != Some(0) {
        return None;
    }
    let map = cmd.as_u64(1)? as u32;
    if map == 0 {
        return None;
    }
    Some((map, cmd.as_u64(2)? as u32, cmd.as_u64(3)? as u32))
}

/// Tile id at layer `z` of `(x, y)` (0 when out of the data array).
fn tile_id(grid: &MapGrid, x: u32, y: u32, z: u32) -> i32 {
    let idx = (z as usize * grid.height as usize + y as usize) * grid.width as usize + x as usize;
    grid.data.get(idx).copied().unwrap_or(0)
}

/// The flag of the topmost non-star tile layer at `(x, y)` (`Game_Map.checkPassage`
/// skips star `0x10` tiles). `None` when no such layer exists.
fn determining_flag(grid: &MapGrid, flags: &[u32], x: u32, y: u32) -> Option<u32> {
    for z in (0..4).rev() {
        let t = tile_id(grid, x, y, z);
        if t < 0 {
            continue;
        }
        let flag = flags.get(t as usize).copied().unwrap_or(0);
        if flag & FLAG_STAR == 0 {
            return Some(flag);
        }
    }
    None
}

/// Whether standing on `(x, y)` is impassable in all four directions. Conservative:
/// `false` (don't flag) when the grid/flags are unusable or no determining tile is
/// found, so a missing tileset never yields a false soft-lock.
fn fully_blocked(grid: &MapGrid, flags: &[u32], x: u32, y: u32) -> bool {
    if grid.width == 0 || grid.height == 0 || x >= grid.width || y >= grid.height {
        return false;
    }
    // Need the four tile layers present to read passability.
    if grid.data.len() < grid.width as usize * grid.height as usize * 4 {
        return false;
    }
    match determining_flag(grid, flags, x, y) {
        Some(flag) => (flag & FLAG_ALL_DIRECTIONS) == FLAG_ALL_DIRECTIONS,
        None => false,
    }
}

/// Resolves the collected transfers + the player start against the tileset flags
/// and pushes a [`BlockedTile`] fact for each fixed destination that lands on a
/// fully-blocked tile.
pub fn resolve(
    b: &mut IrBuilder,
    system: &System,
    collector: &Collector,
    tileset_flags: &FxHashMap<u32, Vec<u32>>,
) {
    let flags_for = |tileset_id: u32| -> &[u32] {
        tileset_flags
            .get(&tileset_id)
            .map(Vec::as_slice)
            .unwrap_or(&[])
    };
    for t in &collector.transfers {
        if let Some(grid) = collector.grids.get(&t.map_id)
            && fully_blocked(grid, flags_for(grid.tileset_id), t.x, t.y)
        {
            b.add_blocked_tile(BlockedTile {
                kind: BlockedTileKind::Transfer,
                map_id: t.map_id,
                x: t.x,
                y: t.y,
                location: t.location.clone(),
            });
        }
    }
    if system.start_map_id != 0
        && let Some(grid) = collector.grids.get(&system.start_map_id)
        && fully_blocked(
            grid,
            flags_for(grid.tileset_id),
            system.start_x,
            system.start_y,
        )
    {
        b.add_blocked_tile(BlockedTile {
            kind: BlockedTileKind::PlayerStart,
            map_id: system.start_map_id,
            x: system.start_x,
            y: system.start_y,
            location: Location::file_only("data/System.json"),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 2x1 grid with the given ground-layer (z=0) tile ids; upper layers empty.
    fn grid(width: u32, height: u32, tileset_id: u32, ground: &[i32]) -> MapGrid {
        let mut data = vec![0i32; (width * height * 6) as usize];
        data[..ground.len()].copy_from_slice(ground);
        MapGrid {
            width,
            height,
            tileset_id,
            data,
        }
    }

    #[test]
    fn wall_tile_is_fully_blocked() {
        // flags[0]=0x10: the empty tile (id 0) is a star (no effect on passage), as
        // in real RPG Maker data, so empty upper layers defer to the ground layer.
        // tile id 1 has flag 0x0f (impassable all directions), tile id 2 is 0 (open).
        let flags = vec![0x10u32, 0x0f, 0x00];
        let g = grid(2, 1, 7, &[1, 2]);
        assert!(fully_blocked(&g, &flags, 0, 0)); // wall tile
        assert!(!fully_blocked(&g, &flags, 1, 0)); // open tile
    }

    #[test]
    fn star_layer_is_skipped_for_the_layer_below() {
        // Upper layer (z=3) is a star tile (0x10) over a wall (0x0f) on z=0.
        // flags[0]=0x10 so the empty z=1/z=2 layers are skipped too.
        let flags = vec![0x10u32, 0x0f, 0x10];
        let mut g = grid(1, 1, 7, &[1]);
        // Put star tile 2 on layer z=3: index (3*height+0)*width+0 = 3.
        g.data[3] = 2;
        assert!(fully_blocked(&g, &flags, 0, 0));
    }

    #[test]
    fn missing_flags_never_blocks() {
        // No flags for the tileset → every tile reads as passable → no false soft-lock.
        let flags: Vec<u32> = Vec::new();
        let g = grid(1, 1, 7, &[1]);
        assert!(!fully_blocked(&g, &flags, 0, 0));
    }

    #[test]
    fn out_of_bounds_never_blocks() {
        let flags = vec![0u32, 0x0f];
        let g = grid(1, 1, 7, &[1]);
        assert!(!fully_blocked(&g, &flags, 5, 5));
    }

    #[test]
    fn direct_transfer_parses_only_direct_designation() {
        let direct = EventCommand {
            code: 201,
            indent: 0,
            parameters: vec![0.into(), 3.into(), 5.into(), 7.into()],
        };
        assert_eq!(direct_transfer(&direct), Some((3, 5, 7)));
        let by_var = EventCommand {
            code: 201,
            indent: 0,
            parameters: vec![1.into(), 3.into(), 5.into(), 7.into()],
        };
        assert_eq!(direct_transfer(&by_var), None);
    }
}
