//! C2 exact-state BFS: identical traversal contract to `legacy`, single-copy
//! retained storage (`StateStore`). A node is still the exact ordered
//! presentation; each outgoing edge is still one official move id 0..=13
//! applied through ac-core; FIFO order plus first-visit-wins still yields
//! deterministic shortest certificates.
//!
//! Deliberately NOT optimized here (S2.1 retains storage, not transitions):
//! successors are still materialized as temporary `Presentation`s through the
//! untouched `apply_move`. Only what is *retained* changed shape.

use super::store::{StateId, StateStore};
use super::{Parent, SearchLimits, SearchMetrics, SearchResult};
use super::{exhausted, failed, limit_reached, reduce_presentation, solved};
use ac_core::{
    Certificate, DEFAULT_N_GEN, Move, Presentation, all_move_ids, apply_move, is_ac_target,
    move_from_id,
};
use std::collections::VecDeque;
use std::time::Instant;

/// Walk the first-discovery parent chain from `end` back to the root and
/// return the move ids in root-to-end order.
fn reconstruct(store: &StateStore, end: StateId) -> Certificate {
    let mut ids = Vec::new();
    let mut cur = end;
    while let Some(parent) = store.parent(cur) {
        ids.push(parent.move_id);
        cur = parent.parent_index;
    }
    ids.reverse();
    ids
}

pub(crate) fn solve_exact_state_bfs(
    start: &Presentation,
    limits: &SearchLimits,
) -> SearchResult {
    let t0 = Instant::now();
    let root = match reduce_presentation(start) {
        Ok(root) => root,
        Err(msg) => return failed(msg, SearchMetrics::default(), t0.elapsed()),
    };
    let mut metrics = SearchMetrics {
        max_total_relator_length: root.total_relator_length(),
        ..SearchMetrics::default()
    };
    // The initial state may already be the exact target: solved, no expansion.
    if is_ac_target(&root) {
        metrics.unique = 1;
        return solved(&root, Vec::new(), metrics, t0.elapsed());
    }
    // Seed the store and the FIFO frontier with the root (id 0).
    let mut store = StateStore::new();
    store.insert_new(&root.relators[0], &root.relators[1], None, 0);
    let mut queue: VecDeque<StateId> = VecDeque::new();
    queue.push_back(0);
    metrics.unique = 1;
    metrics.max_frontier = 1;
    let table: [Move; 14] =
        core::array::from_fn(|i| move_from_id(i as u8).expect("ids 0..=13 are pinned"));
    // Caller-owned expansion buffers: decoded once per pop, capacity reused.
    let (mut w0, mut w1) = (Vec::new(), Vec::new());
    // True once any limit suppressed a state: an unsolved drain is then
    // LimitReached, never Exhausted.
    let mut pruned = false;
    // Pop states FIFO.
    while let Some(si) = queue.pop_front() {
        if limits
            .max_expanded
            .is_some_and(|cap| metrics.expanded >= cap)
        {
            return limit_reached(metrics, t0.elapsed());
        }
        let depth = store.depth(si);
        store.copy_state(si, &mut w0, &mut w1);
        let current = Presentation {
            relators: [std::mem::take(&mut w0), std::mem::take(&mut w1)],
        };
        // Each official move id in order.
        for id in all_move_ids() {
            if limits
                .max_work
                .is_some_and(|cap| metrics.generated >= cap)
            {
                return limit_reached(metrics, t0.elapsed());
            }
            let successor =
                match apply_move(&current, &table[id as usize], DEFAULT_N_GEN) {
                    Ok(p) => p,
                    Err(e) => return failed(e.0, metrics, t0.elapsed()),
                };
            metrics.generated += 1;
            metrics.max_total_relator_length = metrics
                .max_total_relator_length
                .max(successor.total_relator_length());
            // Duplicate suppression: first visit wins and is never overwritten.
            // (Checked before limit pruning, exactly like the oracle: a state
            // that is both visited and over a limit counts as a duplicate.)
            if store.contains(&successor.relators[0], &successor.relators[1]) {
                metrics.duplicates += 1;
                continue;
            }
            let child_depth = depth + 1;
            if limits
                .max_total_relator_length
                .is_some_and(|cap| successor.total_relator_length() > cap)
                || limits.max_depth.is_some_and(|cap| child_depth > cap)
            {
                pruned = true;
                continue;
            }
            // First discovery: record parent + move, then enqueue. The probe
            // above already missed with no store mutation since, so this
            // insert cannot fail.
            let child = store
                .insert_new(
                    &successor.relators[0],
                    &successor.relators[1],
                    Some(Parent {
                        parent_index: si,
                        move_id: id,
                    }),
                    child_depth,
                )
                .expect("probed-absent state must insert");
            metrics.unique += 1;
            metrics.max_depth_seen = metrics.max_depth_seen.max(child_depth);
            // Return at first discovery of the exact target: BFS discovers
            // in nondecreasing depth, so this is a shortest certificate.
            if is_ac_target(&successor) {
                let certificate = reconstruct(&store, child);
                return solved(&root, certificate, metrics, t0.elapsed());
            }
            queue.push_back(child);
            metrics.max_frontier = metrics.max_frontier.max(queue.len() as u64);
        }
        // Frozen metric: a state counts as expanded only once all 14 of its
        // outgoing moves were generated.
        metrics.expanded += 1;
        // Recover the expansion buffers for the next pop (capacity retained).
        let Presentation {
            relators: [b0, b1],
        } = current;
        w0 = b0;
        w1 = b1;
    }
    // The frontier drained. Exhausted only if no limit ever cut the graph.
    if pruned {
        limit_reached(metrics, t0.elapsed())
    } else {
        exhausted(metrics, t0.elapsed())
    }
}
