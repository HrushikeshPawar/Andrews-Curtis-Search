//! S4.1 experiment: bank-probing greedy with immediate-inverse pruning.
//!
//! Same best-first discipline as `greedy` (shared `HeapItem`: total length,
//! then discovery sequence), plus two experiment-only changes:
//!
//! - every generated successor is probed against a [`GoalBank`]; a hit
//!   completes the certificate with the stored suffix and returns at once
//!   (first hit wins). The exact target is still tested first: suffix-0 is
//!   always the shortest completion.
//! - the move that is the immediate inverse of the producing move is never
//!   generated (the root, having no producer, still tries all 14). Each
//!   skipped edge would have been a duplicate, so traversal is unchanged;
//!   only `generated`/`duplicates` shrink. Note the metric consequence:
//!   `expanded` counts states that completed their (possibly pruned) fanout.
//!
//! Frozen `greedy`/`beam`/`search` semantics are untouched; this module is
//! the S4.1 measurement vehicle only.

use super::ball::ReverseBall;
use super::goalbank::GoalBank;
use super::greedy::HeapItem;
use super::store::{StateId, StateStore, reconstruct};
use super::{Parent, SearchLimits, SearchMetrics, SearchResult};
use super::{exhausted, failed, limit_reached, reduce_presentation, solved};
use ac_core::{
    DEFAULT_N_GEN, Move, Presentation, all_move_ids, apply_move, inverse_move, is_ac_target,
    move_from_id, move_to_id,
};
use std::collections::BinaryHeap;
use std::time::Instant;

/// Outcome of a bank-probing run: the standard result, whether the winning
/// certificate came through the atlas bank or the reverse ball (`false` for
/// both = exact target), and the ball-hit depth when applicable.
pub struct AtlasOutcome {
    pub result: SearchResult,
    pub via_bank: bool,
    pub via_ball: bool,
    pub ball_hit_depth: Option<u32>,
}

pub(crate) fn inverse_table() -> [u8; 14] {
    core::array::from_fn(|i| {
        let mv = move_from_id(i as u8).expect("ids 0..=13 are pinned");
        let inv = inverse_move(&mv).expect("every official move inverts");
        move_to_id(&inv).expect("inverses land on official ids")
    })
}

pub(crate) fn solve_bank_greedy(
    start: &Presentation,
    limits: &SearchLimits,
    bank: &GoalBank,
    ball: Option<&mut ReverseBall>,
) -> AtlasOutcome {
    solve_bank_greedy_observed(start, limits, bank, ball, None)
}

/// Discovery observer for instrumentation runs: receives each freshly stored
/// exact state and its discovery depth.
pub type DiscoverObserver<'a> = &'a mut dyn FnMut(&Presentation, u32);

/// Same traversal as `solve_bank_greedy`, additionally reporting every newly
/// discovered exact state and its discovery depth to `on_discover`.
/// Instrumentation hook for the S4.2a quotient probe: the callback sees
/// states in discovery order and cannot influence traversal.
pub(crate) fn solve_bank_greedy_observed(
    start: &Presentation,
    limits: &SearchLimits,
    bank: &GoalBank,
    mut ball: Option<&mut ReverseBall>,
    mut on_discover: Option<DiscoverObserver<'_>>,
) -> AtlasOutcome {
    let done =
        |result: SearchResult, via_bank: bool, via_ball: bool, ball_hit_depth: Option<u32>| {
            AtlasOutcome {
                result,
                via_bank,
                via_ball,
                ball_hit_depth,
            }
        };
    let t0 = Instant::now();
    let root = match reduce_presentation(start) {
        Ok(root) => root,
        Err(msg) => {
            return done(
                failed(msg, SearchMetrics::default(), t0.elapsed()),
                false,
                false,
                None,
            );
        }
    };
    let mut metrics = SearchMetrics {
        max_total_relator_length: root.total_relator_length(),
        ..SearchMetrics::default()
    };
    // The initial state may already be the exact target: solved, no expansion.
    if is_ac_target(&root) {
        metrics.unique = 1;
        return done(
            solved(&root, Vec::new(), metrics, t0.elapsed()),
            false,
            false,
            None,
        );
    }
    // A banked start already knows a suffix: no expansion needed. (The ball
    // holds the root with the empty suffix, but the exact-target check above
    // already covers that case.)
    if let Some(suffix) = bank.suffix(&root) {
        metrics.unique = 1;
        return done(
            solved(&root, suffix.to_vec(), metrics, t0.elapsed()),
            true,
            false,
            None,
        );
    }
    // Seed the store and the heap with the root (id 0).
    let mut store = StateStore::new();
    store.insert_new(&root.relators[0], &root.relators[1], None, 0);
    let mut heap: BinaryHeap<HeapItem> = BinaryHeap::new();
    heap.push(HeapItem {
        length: root.total_relator_length(),
        seq: 0,
        id: 0,
    });
    let mut seq: u64 = 1;
    metrics.unique = 1;
    metrics.max_frontier = 1;
    let table: [Move; 14] =
        core::array::from_fn(|i| move_from_id(i as u8).expect("ids 0..=13 are pinned"));
    let inverse: [u8; 14] = inverse_table();
    // Caller-owned expansion buffers: decoded once per pop, capacity reused.
    let (mut w0, mut w1) = (Vec::new(), Vec::new());
    // True once any limit suppressed a state: an unsolved drain is then
    // LimitReached, never Exhausted.
    let mut pruned = false;
    // Pop best-first.
    while let Some(item) = heap.pop() {
        if limits
            .max_expanded
            .is_some_and(|cap| metrics.expanded >= cap)
        {
            return done(limit_reached(metrics, t0.elapsed()), false, false, None);
        }
        let depth = store.depth(item.id);
        // The edge back to the producer is a certain duplicate: skip it
        // before spending a generated-successor unit. The root has no
        // producer and tries all 14 moves.
        let skip = store.parent(item.id).map(|p| inverse[p.move_id as usize]);
        store.copy_state(item.id, &mut w0, &mut w1);
        let current = Presentation {
            relators: [std::mem::take(&mut w0), std::mem::take(&mut w1)],
        };
        // Each official move id in order, minus the pruned inverse.
        for id in all_move_ids() {
            if Some(id) == skip {
                continue;
            }
            if limits.max_work.is_some_and(|cap| metrics.generated >= cap) {
                return done(limit_reached(metrics, t0.elapsed()), false, false, None);
            }
            let successor = match apply_move(&current, &table[id as usize], DEFAULT_N_GEN) {
                Ok(p) => p,
                Err(e) => {
                    return done(failed(e.0, metrics, t0.elapsed()), false, false, None);
                }
            };
            metrics.generated += 1;
            let length = successor.total_relator_length();
            metrics.max_total_relator_length = metrics.max_total_relator_length.max(length);
            // Permanent suppression: seen once means seen forever.
            if store.contains(&successor.relators[0], &successor.relators[1]) {
                metrics.duplicates += 1;
                continue;
            }
            let child_depth = depth + 1;
            if limits
                .max_total_relator_length
                .is_some_and(|cap| length > cap)
                || limits.max_depth.is_some_and(|cap| child_depth > cap)
            {
                pruned = true;
                continue;
            }
            // First discovery: record parent + move, then push. The probe
            // above already missed with no store mutation since, so this
            // insert cannot fail.
            let child = store
                .insert_new(
                    &successor.relators[0],
                    &successor.relators[1],
                    Some(Parent {
                        parent_index: item.id,
                        move_id: id,
                    }),
                    child_depth,
                )
                .expect("probed-absent state must insert");
            metrics.unique += 1;
            metrics.max_depth_seen = metrics.max_depth_seen.max(child_depth);
            // Instrumentation hook: report the freshly discovered state
            // before any terminating probe can end the run.
            if let Some(cb) = on_discover.as_deref_mut() {
                cb(&successor, child_depth);
            }
            // Exact target first: suffix-0 is always the shortest completion.
            if is_ac_target(&successor) {
                let certificate = reconstruct(&store, child);
                return done(
                    solved(&root, certificate, metrics, t0.elapsed()),
                    false,
                    false,
                    None,
                );
            }
            // Known-suffix hits: the atlas bank and the reverse ball are
            // both exact, so the shorter completion wins (ties go to the
            // ball: its suffix is optimal by construction). Either way the
            // run returns at once with a fully replay-gated certificate.
            let bank_len = bank.suffix(&successor).map(<[u8]>::len);
            let ball_hit: Option<(StateId, u32)> = match ball.as_mut() {
                Some(b) => b
                    .lookup(&successor.relators[0], &successor.relators[1])
                    .map(|hid| (hid, b.depth(hid))),
                None => None,
            };
            let use_ball = match (bank_len, ball_hit) {
                (Some(a), Some((_, b))) => (b as usize) <= a,
                (None, Some(_)) => true,
                _ => false,
            };
            if use_ball {
                let (hid, depth) = ball_hit.expect("selected ball path");
                let suffix = ball.as_ref().expect("ball probed above").suffix(hid);
                let mut certificate = reconstruct(&store, child);
                certificate.extend_from_slice(&suffix);
                return done(
                    solved(&root, certificate, metrics, t0.elapsed()),
                    false,
                    true,
                    Some(depth),
                );
            }
            if let Some(suffix) = bank.suffix(&successor) {
                let mut certificate = reconstruct(&store, child);
                certificate.extend_from_slice(suffix);
                return done(
                    solved(&root, certificate, metrics, t0.elapsed()),
                    true,
                    false,
                    None,
                );
            }
            heap.push(HeapItem {
                length,
                seq,
                id: child,
            });
            seq += 1;
            metrics.max_frontier = metrics.max_frontier.max(heap.len() as u64);
        }
        // A state counts as expanded once its (possibly pruned) fanout
        // completed: every non-skipped move was generated.
        metrics.expanded += 1;
        // Recover the expansion buffers for the next pop (capacity retained).
        let Presentation { relators: [b0, b1] } = current;
        w0 = b0;
        w1 = b1;
    }
    // The frontier drained. Exhausted only if no limit ever cut the graph.
    if pruned {
        done(limit_reached(metrics, t0.elapsed()), false, false, None)
    } else {
        done(exhausted(metrics, t0.elapsed()), false, false, None)
    }
}
