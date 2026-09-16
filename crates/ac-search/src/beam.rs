//! Deterministic beam search (S3.0c): layered best-first over total relator
//! length through the shared `StateStore`, same limits/metrics schema as BFS
//! plus `surviving_width_per_depth`.
//!
//! Frozen v1 policy:
//! - layers: expand every member of the current beam, then keep the top W
//!   fresh states; no carry-over between layers;
//! - score = `|r0| + |r1|`, tie-broken by discovery `StateId` (total order,
//!   reproducible across runs and machines);
//! - global exact visited set with permanent suppression (first visit wins);
//! - goal test on generation (same accounting point as BFS);
//! - `expanded` counts members that generated all 14 moves (BFS mirror).

use super::store::{StateId, StateStore, reconstruct};
use super::{Parent, SearchLimits, SearchMetrics, SearchResult};
use super::{exhausted, failed, limit_reached, reduce_presentation, solved};
use ac_core::{
    DEFAULT_N_GEN, Move, Presentation, all_move_ids, apply_move, is_ac_target, move_from_id,
};
use std::time::Instant;

/// Layered beam search with the given width. Returns the first certificate
/// discovered in deterministic generation order (not shortest guaranteed).
pub(crate) fn solve_beam(
    start: &Presentation,
    limits: &SearchLimits,
    width: usize,
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
    // Seed the store and the beam with the root (id 0, depth 0).
    let mut store = StateStore::new();
    store.insert_new(&root.relators[0], &root.relators[1], None, 0);
    let mut beam: Vec<StateId> = vec![0];
    metrics.unique = 1;
    metrics.max_frontier = 1;
    metrics.surviving_width_per_depth = Some(vec![1]);
    let table: [Move; 14] =
        core::array::from_fn(|i| move_from_id(i as u8).expect("ids 0..=13 are pinned"));
    // Caller-owned expansion buffers: decoded once per member, reused.
    let (mut w0, mut w1) = (Vec::new(), Vec::new());
    // True once any limit suppressed a state: an unsolved drain is then
    // LimitReached, never Exhausted.
    let mut pruned = false;
    // Expand layer by layer. Every beam member has the same depth, so every
    // fresh successor of the layer has depth + 1.
    while !beam.is_empty() {
        let depth = store.depth(beam[0]);
        // (length, discovery id) in generation order; truncated to width.
        let mut candidates: Vec<(usize, StateId)> = Vec::new();
        for &si in &beam {
            if limits
                .max_expanded
                .is_some_and(|cap| metrics.expanded >= cap)
            {
                return limit_reached(metrics, t0.elapsed());
            }
            store.copy_state(si, &mut w0, &mut w1);
            let current = Presentation {
                relators: [std::mem::take(&mut w0), std::mem::take(&mut w1)],
            };
            // Each official move id in order.
            for id in all_move_ids() {
                if limits.max_work.is_some_and(|cap| metrics.generated >= cap) {
                    return limit_reached(metrics, t0.elapsed());
                }
                let successor = match apply_move(&current, &table[id as usize], DEFAULT_N_GEN) {
                    Ok(p) => p,
                    Err(e) => return failed(e.0, metrics, t0.elapsed()),
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
                // First discovery: record parent + move. The probe above
                // already missed with no store mutation since, so this
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
                // Return at first discovery of the exact target.
                if is_ac_target(&successor) {
                    let certificate = reconstruct(&store, child);
                    return solved(&root, certificate, metrics, t0.elapsed());
                }
                candidates.push((length, child));
            }
            // Frozen metric: a member counts as expanded only once all 14 of
            // its outgoing moves were generated.
            metrics.expanded += 1;
            // Recover the expansion buffers for the next member.
            let Presentation { relators: [b0, b1] } = current;
            w0 = b0;
            w1 = b1;
        }
        // Deterministic truncation: total length, then discovery order.
        // Frontier records the largest candidate pool considered; the widths
        // vector records what survived truncation.
        metrics.max_frontier = metrics.max_frontier.max(candidates.len() as u64);
        candidates.sort();
        candidates.truncate(width);
        if let Some(widths) = metrics.surviving_width_per_depth.as_mut() {
            widths.push(candidates.len() as u32);
        }
        beam = candidates.into_iter().map(|(_, id)| id).collect();
    }
    // The beam drained. Exhausted only if no limit ever cut the graph.
    if pruned {
        limit_reached(metrics, t0.elapsed())
    } else {
        exhausted(metrics, t0.elapsed())
    }
}
