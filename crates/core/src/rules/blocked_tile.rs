//! Rule `blocked-tile`: a fixed destination that lands on an impassable tile.
//!
//! Consumes the adapter's spatial facts ([`crate::ir::Ir::blocked_tiles`]): a
//! Transfer Player (201) with a literal map+x+y, or the player's start position
//! (System.json), whose target tile is impassable from **all four** directions per
//! the tileset passage flags. Standing there, the player cannot move in any
//! direction — a soft-lock.
//!
//! Confidence `likely`: passability plugins (region passage, pixel-movement) and
//! through-events are not accounted for, and the spatial pass reads only the
//! literal, statically-known coordinates. The rule is off by default (opt-in via
//! `--tiles`) because passability-modifying plugins are common.

use crate::finding::{Category, Confidence, Finding, Severity};
use crate::ir::BlockedTileKind;
use crate::message::Msg;
use crate::rules::{Rule, RuleCtx};

/// Rule that flags transfers / the player start landing on a fully-blocked tile.
pub struct BlockedTile;

impl Rule for BlockedTile {
    fn id(&self) -> &'static str {
        "blocked-tile"
    }

    fn category(&self) -> Category {
        Category::Reference
    }

    fn run(&self, ctx: &RuleCtx<'_>) -> Vec<Finding> {
        ctx.ir
            .blocked_tiles
            .iter()
            .map(|t| {
                let message = match t.kind {
                    BlockedTileKind::Transfer => Msg::TransferToBlockedTile {
                        map_id: t.map_id,
                        x: t.x,
                        y: t.y,
                    },
                    BlockedTileKind::PlayerStart => Msg::StartInWall {
                        map_id: t.map_id,
                        x: t.x,
                        y: t.y,
                    },
                };
                Finding {
                    severity: Severity::Warning,
                    category: Category::Reference,
                    confidence: Confidence::Likely,
                    location: t.location.clone(),
                    message,
                    references: Vec::new(),
                    rule: "blocked-tile",
                }
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{BlockedTile as BlockedTileFact, Engine, Ir, Location};

    #[test]
    fn emits_transfer_and_start_findings() {
        let mut b = Ir::builder(Engine::Mz);
        b.add_blocked_tile(BlockedTileFact {
            kind: BlockedTileKind::Transfer,
            map_id: 3,
            x: 5,
            y: 7,
            location: Location::file_only("data/Map001.json"),
        });
        b.add_blocked_tile(BlockedTileFact {
            kind: BlockedTileKind::PlayerStart,
            map_id: 1,
            x: 0,
            y: 0,
            location: Location::file_only("data/System.json"),
        });
        let ir = b.finish();
        let f = BlockedTile.run(&RuleCtx::new(&ir));
        assert_eq!(f.len(), 2);
        assert!(f.iter().all(|x| x.severity == Severity::Warning));
        assert!(f.iter().all(|x| x.confidence == Confidence::Likely));
        assert!(f.iter().any(|x| matches!(
            x.message,
            Msg::TransferToBlockedTile {
                map_id: 3,
                x: 5,
                y: 7
            }
        )));
        assert!(f.iter().any(|x| matches!(
            x.message,
            Msg::StartInWall {
                map_id: 1,
                x: 0,
                y: 0
            }
        )));
    }

    #[test]
    fn no_facts_no_findings() {
        let ir = Ir::builder(Engine::Mz).finish();
        assert!(BlockedTile.run(&RuleCtx::new(&ir)).is_empty());
    }
}
