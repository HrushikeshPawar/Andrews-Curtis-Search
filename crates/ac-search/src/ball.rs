//! Exact reverse ball (S4.1c): breadth-first expansion outward from the
//! exact target once, shared across all cases.
//!
//! Moves are invertible, so BFS from `[[1],[2]]` with the standard 14 moves
//! visits states in true target-distance order. Every ball state carries an
//! optimal suffix: the inverses of its root path, reversed. Suffix length
//! always equals the state's recorded depth.
//!
//! The ball is membership-only for traversal (never iterated), so hash
//! layout cannot affect determinism. Suffixes materialize on hits only.

use super::Parent;
use super::atlas_search::inverse_table;
use super::store::{StateId, StateStore};
use ac_core::{DEFAULT_N_GEN, Move, MoveId, Presentation, all_move_ids, apply_move, move_from_id};
use std::collections::VecDeque;

pub struct ReverseBall {
    store: StateStore,
    max_depth: u32,
}

impl ReverseBall {
    /// Expand the full ball out to (and including) `max_depth`.
    pub fn build(max_depth: u32) -> Self {
        let mut store = StateStore::new();
        store.insert_new(&[1], &[2], None, 0);
        let table: [Move; 14] =
            core::array::from_fn(|i| move_from_id(i as u8).expect("ids 0..=13 are pinned"));
        let mut queue: VecDeque<StateId> = VecDeque::new();
        queue.push_back(0);
        let (mut w0, mut w1) = (Vec::new(), Vec::new());
        while let Some(si) = queue.pop_front() {
            let depth = store.depth(si);
            if depth >= max_depth {
                continue;
            }
            store.copy_state(si, &mut w0, &mut w1);
            let current = Presentation {
                relators: [std::mem::take(&mut w0), std::mem::take(&mut w1)],
            };
            for id in all_move_ids() {
                let successor = apply_move(&current, &table[id as usize], DEFAULT_N_GEN)
                    .expect("official moves apply to reduced states");
                if store.contains(&successor.relators[0], &successor.relators[1]) {
                    continue;
                }
                let child = store
                    .insert_new(
                        &successor.relators[0],
                        &successor.relators[1],
                        Some(Parent {
                            parent_index: si,
                            move_id: id,
                        }),
                        depth + 1,
                    )
                    .expect("probed-absent state must insert");
                queue.push_back(child);
            }
            let Presentation { relators: [b0, b1] } = current;
            w0 = b0;
            w1 = b1;
        }
        Self { store, max_depth }
    }

    pub fn len(&self) -> u32 {
        self.store.len()
    }

    pub fn is_empty(&self) -> bool {
        self.store.len() == 0
    }

    pub fn max_depth(&self) -> u32 {
        self.max_depth
    }

    /// Membership probe returning the ball id (optimal suffix length for a
    /// hit equals the id's recorded depth; use `suffix` to materialize it).
    pub fn lookup(&mut self, r0: &[i8], r1: &[i8]) -> Option<StateId> {
        self.store.find(r0, r1)
    }

    pub fn depth(&self, id: StateId) -> u32 {
        self.store.depth(id)
    }

    /// Materialize a ball state into caller-owned buffers.
    pub fn copy_state(&self, id: StateId, out0: &mut Vec<i8>, out1: &mut Vec<i8>) {
        self.store.copy_state(id, out0, out1);
    }

    /// Optimal suffix from a ball state to the exact target.
    pub fn suffix(&self, id: StateId) -> Vec<MoveId> {
        let inverse = inverse_table();
        let mut path = Vec::new();
        let mut cur = id;
        while let Some(parent) = self.store.parent(cur) {
            path.push(parent.move_id);
            cur = parent.parent_index;
        }
        path.reverse();
        path.into_iter()
            .map(|m| inverse[m as usize])
            .rev()
            .collect()
    }
}

#[cfg(test)]
mod ball_tests {
    use super::*;
    use ac_core::{is_ac_target, replay_certificate};

    #[test]
    fn ball_1_holds_root_plus_10_distinct_successors() {
        // 4 of the 14 moves self-loop on the target (conjugating [x] by x^±1
        // or [y] by y^±1 fixes the word), so only 10 fresh states appear.
        let ball = ReverseBall::build(1);
        assert_eq!(ball.len(), 11);
        assert_eq!(ball.max_depth(), 1);
    }

    #[test]
    fn every_ball_2_suffix_is_optimal_and_replays() {
        let ball = ReverseBall::build(2);
        println!("pin_ball_2: len={}", ball.len());
        let (mut w0, mut w1) = (Vec::new(), Vec::new());
        for id in 0..ball.len() {
            let depth = ball.depth(id);
            assert!(depth <= 2);
            let suffix = ball.suffix(id);
            // BFS discovery order makes depth the true distance: the
            // materialized suffix must match it exactly and replay.
            assert_eq!(suffix.len(), depth as usize, "non-optimal suffix at {id}");
            ball.copy_state(id, &mut w0, &mut w1);
            let state = Presentation {
                relators: [w0.clone(), w1.clone()],
            };
            let end = replay_certificate(&state, &suffix).unwrap();
            assert!(is_ac_target(&end), "suffix misses target at {id}");
        }
    }
}
