//! Dual-storage exact-state BFS: arena `Vec<Presentation>` plus an owned
//! `HashMap<Presentation, u32>` visited map (two copies of every state).
//!
//! This is the trusted implementation behind all pinned metric rows and the
//! 21 verified sweep certificates. During the S2.1 C2 migration it serves as
//! the differential oracle: the C2 store must reproduce its statuses,
//! certificates, and metrics exactly (elapsed time excepted).

use super::{Parent, SearchLimits, SearchMetrics, SearchResult, SearchStatus};
use ac_core::{
    Certificate, DEFAULT_N_GEN, Move, Presentation, all_move_ids, apply_move, is_ac_target,
    move_from_id, reduce, replay_certificate,
};
use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};

/// Freely reduce a presentation through the trusted kernel, exactly like
/// `replay_certificate` does on entry, so the root state is canonical.
fn reduce_presentation(p: &Presentation) -> Result<Presentation, String> {
    Ok(Presentation {
        relators: [
            reduce(&p.relators[0], DEFAULT_N_GEN).map_err(|e| e.0)?,
            reduce(&p.relators[1], DEFAULT_N_GEN).map_err(|e| e.0)?,
        ],
    })
}
fn failed(msg: String, mut metrics: SearchMetrics, elapsed: Duration) -> SearchResult {
    metrics.elapsed = elapsed;
    SearchResult {
        status: SearchStatus::Failed(msg),
        certificate: Vec::new(),
        certificate_length: None,
        metrics,
    }
}
/// A run is only reported Solved after its certificate replays to the exact
/// target through the trusted kernel; a replay mismatch is a loud failure.
fn solved(
    root: &Presentation,
    certificate: Certificate,
    mut metrics: SearchMetrics,
    elapsed: Duration,
) -> SearchResult {
    metrics.elapsed = elapsed;
    metrics.replay_ok = replay_certificate(root, &certificate).is_ok();
    if metrics.replay_ok {
        SearchResult {
            status: SearchStatus::Solved,
            certificate_length: Some(certificate.len()),
            certificate,
            metrics,
        }
    } else {
        failed("replay_failed_after_reconstruction".into(), metrics, elapsed)
    }
}
/// Walk the first-discovery parent chain from `end` back to the root and
/// return the move ids in root-to-end order.
fn reconstruct(parents: &[Option<Parent>], end: u32) -> Certificate {
    let mut ids = Vec::new();
    let mut cur = end;
    while let Some(parent) = parents[cur as usize] {
        ids.push(parent.move_id);
        cur = parent.parent_index;
    }
    ids.reverse();
    ids
}
fn limit_reached(mut metrics: SearchMetrics, elapsed: Duration) -> SearchResult {
    metrics.elapsed = elapsed;
    SearchResult {
        status: SearchStatus::LimitReached,
        certificate: Vec::new(),
        certificate_length: None,
        metrics,
    }
}
fn exhausted(mut metrics: SearchMetrics, elapsed: Duration) -> SearchResult {
    metrics.elapsed = elapsed;
    SearchResult {
        status: SearchStatus::Exhausted,
        certificate: Vec::new(),
        certificate_length: None,
        metrics,
    }
}
/// Exact-state BFS. A node is the exact ordered presentation; each outgoing
/// edge is one official move id 0..=13 applied through ac-core. No cyclic
/// reduction, no canonicalization, no relator swapping, no heuristic.
/// Successors are tried in id order and states are marked visited at
/// discovery (first visit wins), so certificates are deterministic.
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
    // Seed the visited map and the FIFO frontier with the root (arena index 0).
    let mut states: Vec<Presentation> = vec![root.clone()];
    let mut parents: Vec<Option<Parent>> = vec![None];
    let mut depths: Vec<u32> = vec![0];
    let mut visited: HashMap<Presentation, u32> = HashMap::new();
    visited.insert(root.clone(), 0);
    let mut queue: VecDeque<u32> = VecDeque::new();
    queue.push_back(0);
    metrics.unique = 1;
    metrics.max_frontier = 1;
    let table: [Move; 14] =
        core::array::from_fn(|i| move_from_id(i as u8).expect("ids 0..=13 are pinned"));
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
        let depth = depths[si as usize];
        // Each official move id in order.
        for id in all_move_ids() {
            if limits
                .max_work
                .is_some_and(|cap| metrics.generated >= cap)
            {
                return limit_reached(metrics, t0.elapsed());
            }
            let successor =
                match apply_move(&states[si as usize], &table[id as usize], DEFAULT_N_GEN) {
                    Ok(p) => p,
                    Err(e) => return failed(e.0, metrics, t0.elapsed()),
                };
            metrics.generated += 1;
            metrics.max_total_relator_length = metrics
                .max_total_relator_length
                .max(successor.total_relator_length());
            // Duplicate suppression: first visit wins and is never overwritten.
            if visited.contains_key(&successor) {
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
            // First discovery: record parent + move, then enqueue.
            let child = states.len() as u32;
            states.push(successor.clone());
            parents.push(Some(Parent {
                parent_index: si,
                move_id: id,
            }));
            depths.push(child_depth);
            visited.insert(successor, child);
            metrics.unique += 1;
            metrics.max_depth_seen = metrics.max_depth_seen.max(child_depth);
            // Return at first discovery of the exact target: BFS discovers
            // in nondecreasing depth, so this is a shortest certificate.
            if is_ac_target(&states[child as usize]) {
                let certificate = reconstruct(&parents, child);
                return solved(&root, certificate, metrics, t0.elapsed());
            }
            queue.push_back(child);
            metrics.max_frontier = metrics.max_frontier.max(queue.len() as u64);
        }
        // Frozen metric: a state counts as expanded only once all 14 of its
        // outgoing moves were generated.
        metrics.expanded += 1;
    }
    // The frontier drained. Exhausted only if no limit ever cut the graph.
    if pruned {
        limit_reached(metrics, t0.elapsed())
    } else {
        exhausted(metrics, t0.elapsed())
    }
}
