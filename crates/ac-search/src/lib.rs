//! Search infrastructure. Exact-state BFS over the ordinary AC graph.
use ac_core::{
    Certificate, DEFAULT_N_GEN, Presentation, is_ac_target, reduce, replay_certificate,
};
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchLimits {
    pub max_expanded: Option<u64>,
    pub max_depth: Option<u32>,
    pub max_total_relator_length: Option<usize>,
    pub max_work: Option<u64>,
}
impl Default for SearchLimits {
    fn default() -> Self {
        Self {
            max_expanded: Some(1_000_000),
            max_depth: None,
            max_total_relator_length: Some(10_000),
            max_work: Some(5_000_000),
        }
    }
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SearchMetrics {
    pub expanded: u64,
    pub generated: u64,
    pub unique: u64,
    pub duplicates: u64,
    pub max_frontier: u64,
    pub max_depth_seen: u32,
    pub max_total_relator_length: usize,
    pub elapsed: Duration,
    pub replay_ok: bool,
    pub official_verifier_status: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SearchStatus {
    Solved,
    Exhausted,
    LimitReached,
    Unimplemented,
    Failed(String),
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub status: SearchStatus,
    pub certificate: Certificate,
    pub certificate_length: Option<usize>,
    pub metrics: SearchMetrics,
}
impl SearchResult {
    pub fn unimplemented() -> Self {
        Self {
            status: SearchStatus::Unimplemented,
            certificate: Vec::new(),
            certificate_length: None,
            metrics: SearchMetrics::default(),
        }
    }
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Parent {
    pub parent_index: u32,
    pub move_id: u8,
}
pub trait ExactStateSolver {
    fn solve(&self, start: &Presentation, limits: &SearchLimits) -> SearchResult;
}
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
/// Exact-state BFS. A node is the exact ordered presentation; each outgoing
/// edge is one official move id 0..=13 applied through ac-core. No cyclic
/// reduction, no canonicalization, no relator swapping, no heuristic.
pub fn solve_exact_state_bfs(start: &Presentation, _limits: &SearchLimits) -> SearchResult {
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
    // The FIFO search loop is the next step of the walkthrough.
    SearchResult::unimplemented()
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExperimentRecord {
    pub format_version: u32,
    pub challenge_id: String,
    pub algorithm: String,
    pub git_commit: String,
    pub sair_commit: String,
    pub move_spec_version: String,
    pub move_spec_hash: String,
    pub limits: SearchLimits,
    pub metrics: SearchMetrics,
    pub certificate: Certificate,
    pub certificate_length: Option<usize>,
    pub verification_status: Option<String>,
}

#[cfg(test)]
mod bfs_tests {
    use super::*;

    fn pres(r0: &[i8], r1: &[i8]) -> Presentation {
        Presentation {
            relators: [r0.to_vec(), r1.to_vec()],
        }
    }

    #[test]
    fn depth0_exact_target_solves_with_empty_certificate() {
        let result = solve_exact_state_bfs(&pres(&[1], &[2]), &SearchLimits::default());
        assert!(matches!(result.status, SearchStatus::Solved));
        assert_eq!(result.certificate, Vec::<u8>::new());
        assert_eq!(result.certificate_length, Some(0));
        assert!(result.metrics.replay_ok);
        assert_eq!(result.metrics.expanded, 0);
        assert_eq!(result.metrics.generated, 0);
        assert_eq!(result.metrics.unique, 1);
        assert_eq!(result.metrics.duplicates, 0);
        assert_eq!(result.metrics.max_frontier, 0);
        assert_eq!(result.metrics.max_depth_seen, 0);
        assert_eq!(result.metrics.max_total_relator_length, 2);
    }

    #[test]
    fn depth0_unreduced_root_matches_official_replay_semantics() {
        // Freely equivalent to the target: reducing first (like replay does)
        // means this is solved with the empty certificate.
        let start = pres(&[1, 2, -2], &[2]);
        let result = solve_exact_state_bfs(&start, &SearchLimits::default());
        assert!(matches!(result.status, SearchStatus::Solved));
        assert_eq!(result.certificate, Vec::<u8>::new());
        assert_eq!(result.metrics.expanded, 0);
        assert_eq!(result.metrics.unique, 1);
        assert!(replay_certificate(&start, &[]).is_ok());
    }

    #[test]
    fn invalid_letters_fail_honestly() {
        let result = solve_exact_state_bfs(&pres(&[3], &[2]), &SearchLimits::default());
        assert!(matches!(result.status, SearchStatus::Failed(_)));
        assert_eq!(result.metrics.expanded, 0);
        assert_eq!(result.metrics.unique, 0);
        assert!(!result.metrics.replay_ok);
    }
}
