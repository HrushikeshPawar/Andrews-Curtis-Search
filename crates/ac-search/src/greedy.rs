//! Deterministic length-greedy search (S3.0b): best-first over total relator
//! length through the shared `StateStore`, same limits/metrics schema as BFS.
//!
//! Frozen v1 policy:
//! - priority = `|r0| + |r1|`, tie-broken by discovery sequence (total order,
//!   reproducible across runs and machines);
//! - exact visited set with permanent suppression: a state is recorded once
//!   at first discovery and never reopened, so no heap entry ever goes stale
//!   and no key ever decreases;
//! - goal test on generation (same accounting point as BFS);
//! - `expanded` counts popped states that generated all 14 moves (BFS mirror).

use super::store::{StateId, StateStore, reconstruct};
use super::{Parent, SearchLimits, SearchMetrics, SearchResult};
use super::{exhausted, failed, limit_reached, reduce_presentation, solved};
use ac_core::{
    DEFAULT_N_GEN, Move, Presentation, all_move_ids, apply_move, is_ac_target, move_from_id,
};
use std::cmp::Ordering;
use std::collections::BinaryHeap;
use std::time::Instant;

#[derive(Eq, PartialEq)]
struct HeapItem {
    length: usize,
    seq: u64,
    id: StateId,
}

impl Ord for HeapItem {
    // BinaryHeap is a max-heap: reverse the comparison so the smallest
    // (length, discovery sequence) pops first.
    fn cmp(&self, other: &Self) -> Ordering {
        other
            .length
            .cmp(&self.length)
            .then(other.seq.cmp(&self.seq))
    }
}

impl PartialOrd for HeapItem {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// Length-greedy certificate search. Returns a (non-guaranteed-shortest)
/// certificate on first discovery of the exact target.
pub(crate) fn solve_length_greedy(
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
            return limit_reached(metrics, t0.elapsed());
        }
        let depth = store.depth(item.id);
        store.copy_state(item.id, &mut w0, &mut w1);
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
            let length = successor.total_relator_length();
            metrics.max_total_relator_length =
                metrics.max_total_relator_length.max(length);
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
            // Return at first discovery of the exact target.
            if is_ac_target(&successor) {
                let certificate = reconstruct(&store, child);
                return solved(&root, certificate, metrics, t0.elapsed());
            }
            heap.push(HeapItem {
                length,
                seq,
                id: child,
            });
            seq += 1;
            metrics.max_frontier = metrics.max_frontier.max(heap.len() as u64);
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
