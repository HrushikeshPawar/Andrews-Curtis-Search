//! Search infrastructure. Exact-state BFS over the ordinary AC graph.
use ac_core::{Certificate, DEFAULT_N_GEN, Presentation, reduce, replay_certificate};
use serde::{Deserialize, Serialize};
use std::time::Duration;

mod portfolio;
mod search;
mod store;

pub use portfolio::{Budget, CaseReport};

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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Parent {
    pub parent_index: u32,
    pub move_id: u8,
}
pub trait ExactStateSolver {
    fn solve(&self, start: &Presentation, limits: &SearchLimits) -> SearchResult;
}
/// Freely reduce a presentation through the trusted kernel, exactly like
/// `replay_certificate` does on entry, so the root state is canonical.
pub(crate) fn reduce_presentation(p: &Presentation) -> Result<Presentation, String> {
    Ok(Presentation {
        relators: [
            reduce(&p.relators[0], DEFAULT_N_GEN).map_err(|e| e.0)?,
            reduce(&p.relators[1], DEFAULT_N_GEN).map_err(|e| e.0)?,
        ],
    })
}
pub(crate) fn failed(
    msg: String,
    mut metrics: SearchMetrics,
    elapsed: Duration,
) -> SearchResult {
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
pub(crate) fn solved(
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
pub(crate) fn limit_reached(mut metrics: SearchMetrics, elapsed: Duration) -> SearchResult {
    metrics.elapsed = elapsed;
    SearchResult {
        status: SearchStatus::LimitReached,
        certificate: Vec::new(),
        certificate_length: None,
        metrics,
    }
}
pub(crate) fn exhausted(mut metrics: SearchMetrics, elapsed: Duration) -> SearchResult {
    metrics.elapsed = elapsed;
    SearchResult {
        status: SearchStatus::Exhausted,
        certificate: Vec::new(),
        certificate_length: None,
        metrics,
    }
}
/// Exact-state BFS over the official move graph (see module `search` for the
/// traversal contract): exact ordered presentations, official move ids
/// 0..=13 in order, FIFO with first-visit-wins duplicate suppression.
pub fn solve_exact_state_bfs(start: &Presentation, limits: &SearchLimits) -> SearchResult {
    search::solve_exact_state_bfs(start, limits)
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
mod bfs_tests {use super::*;
    use ac_core::replay_certificate;

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

    #[test]
    fn depth1_xy_y_solves_with_move_3() {
        // <x,y | xy, y>: move 3 (r0 <- r0 * r1^-1) is the first official move
        // that reaches [[1],[2]], so the shortest certificate is [3].
        let result = solve_exact_state_bfs(&pres(&[1, 2], &[2]), &SearchLimits::default());
        assert!(matches!(result.status, SearchStatus::Solved));
        assert_eq!(result.certificate, vec![3]);
        assert_eq!(result.certificate_length, Some(1));
        assert!(result.metrics.replay_ok);
        // Frozen definitions: the root's expansion stopped at move 3 when the
        // goal was discovered, so no state completed all 14 moves.
        assert_eq!(result.metrics.expanded, 0);
        assert_eq!(result.metrics.generated, 4);
        assert_eq!(result.metrics.unique, 5);
        assert_eq!(result.metrics.duplicates, 0);
        // Max queue size: the root is popped before its children are pushed,
        // so the peak is the 3 new states queued at ids 0..=2.
        assert_eq!(result.metrics.max_frontier, 3);
        assert_eq!(result.metrics.max_depth_seen, 1);
        assert_eq!(result.metrics.max_total_relator_length, 4);
    }

    #[test]
    fn back_1_solves_with_move_5() {
        // Orientation fixture back_1.
        let result = solve_exact_state_bfs(&pres(&[1], &[2, 1]), &SearchLimits::default());
        assert!(matches!(result.status, SearchStatus::Solved));
        assert_eq!(result.certificate, vec![5]);
        assert!(result.metrics.replay_ok);
        assert_eq!(result.metrics.expanded, 0);
        assert_eq!(result.metrics.generated, 6);
        assert_eq!(result.metrics.unique, 7);
        assert_eq!(result.metrics.duplicates, 0);
        assert_eq!(result.metrics.max_frontier, 5);
        assert_eq!(result.metrics.max_depth_seen, 1);
        assert_eq!(result.metrics.max_total_relator_length, 5);
    }

    #[test]
    fn back_3_solves_with_move_4() {
        // Orientation fixture back_3.
        let result = solve_exact_state_bfs(&pres(&[1], &[2, -1]), &SearchLimits::default());
        assert!(matches!(result.status, SearchStatus::Solved));
        assert_eq!(result.certificate, vec![4]);
        assert!(result.metrics.replay_ok);
        assert_eq!(result.metrics.expanded, 0);
        assert_eq!(result.metrics.generated, 5);
        assert_eq!(result.metrics.unique, 6);
        assert_eq!(result.metrics.duplicates, 0);
        assert_eq!(result.metrics.max_frontier, 4);
        assert_eq!(result.metrics.max_depth_seen, 1);
        assert_eq!(result.metrics.max_total_relator_length, 5);
    }

    #[test]
    fn back_4_hits_max_expanded_limit() {
        // Expansion budget binds at pop 51: exactly 50 states completed all
        // 14 moves, so generated is exactly 50 * 14.
        let limits = SearchLimits {
            max_expanded: Some(50),
            max_depth: None,
            max_total_relator_length: None,
            max_work: None,
        };
        let result = solve_exact_state_bfs(&pres(&[-1, -2], &[-1, -2, 1]), &limits);
        assert!(matches!(result.status, SearchStatus::LimitReached));
        assert_eq!(result.certificate, Vec::<u8>::new());
        assert_eq!(result.certificate_length, None);
        assert!(!result.metrics.replay_ok);
        assert_eq!(result.metrics.expanded, 50);
        assert_eq!(result.metrics.generated, 700);
        // Pinned from the deterministic run; identity generated == unique - 1
        // + duplicates holds exactly (nothing was pruned here).
        assert_eq!(result.metrics.unique, 474);
        assert_eq!(result.metrics.duplicates, 227);
        assert_eq!(result.metrics.max_frontier, 424);
        assert_eq!(result.metrics.max_depth_seen, 3);
        assert_eq!(result.metrics.max_total_relator_length, 17);
    }

    #[test]
    fn back_4_hits_max_work_mid_expansion() {
        // Work budget binds inside expansion 51: 50 full expansions (700
        // generated) plus 5 more moves before the pre-application check stops.
        let limits = SearchLimits {
            max_expanded: None,
            max_depth: None,
            max_total_relator_length: None,
            max_work: Some(705),
        };
        let result = solve_exact_state_bfs(&pres(&[-1, -2], &[-1, -2, 1]), &limits);
        assert!(matches!(result.status, SearchStatus::LimitReached));
        assert_eq!(result.metrics.expanded, 50);
        assert_eq!(result.metrics.generated, 705);
        assert_eq!(result.certificate, Vec::<u8>::new());
    }

    #[test]
    fn back_4_max_depth_prunes_and_reports_limit_reached() {
        // Depth cap suppresses every depth-3 child at generation; the run
        // drains but must NOT claim the graph was exhausted.
        let limits = SearchLimits {
            max_expanded: None,
            max_depth: Some(2),
            max_total_relator_length: None,
            max_work: None,
        };
        let result = solve_exact_state_bfs(&pres(&[-1, -2], &[-1, -2, 1]), &limits);
        assert!(matches!(result.status, SearchStatus::LimitReached));
        assert_eq!(result.metrics.max_depth_seen, 2);
        assert_eq!(result.certificate, Vec::<u8>::new());
        // Pinned from the deterministic run: every depth<=2 state was fully
        // expanded (generated == 14 * expanded) and the depth-3 children were
        // pruned at generation, so the drain reports LimitReached.
        assert_eq!(result.metrics.expanded, 140);
        assert_eq!(result.metrics.generated, 1960);
        assert_eq!(result.metrics.unique, 140);
        assert_eq!(result.metrics.duplicates, 273);
        assert_eq!(result.metrics.max_frontier, 126);
        assert_eq!(result.metrics.max_total_relator_length, 19);
    }

    #[test]
    fn empty_relators_component_is_exhausted() {
        // [[],[]] is the canonical finite component: every official move is a
        // self-loop, so the frontier drains with no pruning: honest Exhausted.
        let result = solve_exact_state_bfs(&pres(&[], &[]), &SearchLimits::default());
        assert!(matches!(result.status, SearchStatus::Exhausted));
        assert_eq!(result.certificate, Vec::<u8>::new());
        assert_eq!(result.certificate_length, None);
        assert_eq!(result.metrics.expanded, 1);
        assert_eq!(result.metrics.generated, 14);
        assert_eq!(result.metrics.unique, 1);
        assert_eq!(result.metrics.duplicates, 14);
        assert_eq!(result.metrics.max_frontier, 1);
        assert_eq!(result.metrics.max_depth_seen, 0);
        assert_eq!(result.metrics.max_total_relator_length, 0);
        assert!(!result.metrics.replay_ok);
    }

    #[test]
    fn back_2_solves_with_orientation_certificate() {
        // Orientation fixture back_2: depth 2, orientation reported 10
        // expansions counting the goal-parent's cut-short expansion; the
        // frozen metric counts only its 9 full expansions.
        let result = solve_exact_state_bfs(&pres(&[-2, 1, 2], &[1, 2, -1]), &SearchLimits::default());
        assert!(matches!(result.status, SearchStatus::Solved));
        assert_eq!(result.certificate, vec![8, 11]);
        assert_eq!(result.certificate_length, Some(2));
        assert!(result.metrics.replay_ok);
        assert_eq!(result.metrics.expanded, 9);
        assert_eq!(result.metrics.generated, 138);
        assert_eq!(result.metrics.unique, 117);
        assert_eq!(result.metrics.duplicates, 22);
        assert_eq!(result.metrics.max_depth_seen, 2);
        // Pinned from the deterministic run (orientation recorded no
        // max_frontier or max_total_relator_length for the small cases).
        assert_eq!(result.metrics.max_frontier, 106);
        assert_eq!(result.metrics.max_total_relator_length, 15);
    }

    #[test]
    fn back_4_solves_with_orientation_certificate() {
        // Orientation fixture back_4: depth 4, orientation reported 142
        // expansions; the frozen metric counts 141 full expansions.
        let result = solve_exact_state_bfs(&pres(&[-1, -2], &[-1, -2, 1]), &SearchLimits::default());
        assert!(matches!(result.status, SearchStatus::Solved));
        assert_eq!(result.certificate, vec![0, 1, 3, 10]);
        assert_eq!(result.certificate_length, Some(4));
        assert!(result.metrics.replay_ok);
        assert_eq!(result.metrics.expanded, 141);
        assert_eq!(result.metrics.generated, 1985);
        assert_eq!(result.metrics.unique, 1263);
        assert_eq!(result.metrics.duplicates, 723);
        assert_eq!(result.metrics.max_depth_seen, 4);
        // Pinned from the deterministic run.
        assert_eq!(result.metrics.max_frontier, 1120);
        assert_eq!(result.metrics.max_total_relator_length, 19);
    }

    /// First serious regression. Orientation (release, 2026-09) recorded:
    /// depth 6, certificate [9,10,2,0,8,4], 93,117 expansions counted at pop
    /// time, 1,303,629 generated, 828,690 unique, 474,940 duplicates, max
    /// total relator length 131, ~234 MB peak RSS. The frozen metric counts
    /// only full expansions, so 93,116 here.
    /// Run with: cargo test --release -p ac-search -- --ignored
    #[test]
    #[ignore = "large: ~93k expansions, ~1.3M moves, ~234 MB"]
    fn ms1y_solves_at_depth_6() {
        let limits = SearchLimits {
            max_expanded: Some(200_000),
            max_depth: None,
            max_total_relator_length: Some(10_000),
            max_work: Some(2_000_000),
        };
        let result = solve_exact_state_bfs(&pres(&[-1, 2, 1, -2, -2], &[-1, 2]), &limits);
        assert!(matches!(result.status, SearchStatus::Solved), "{:?}", result.status);
        assert_eq!(result.certificate, vec![9, 10, 2, 0, 8, 4]);
        assert_eq!(result.certificate_length, Some(6));
        assert!(result.metrics.replay_ok);
        assert_eq!(result.metrics.expanded, 93_116);
        assert_eq!(result.metrics.generated, 1_303_629);
        assert_eq!(result.metrics.unique, 828_690);
        assert_eq!(result.metrics.duplicates, 474_940);
        assert_eq!(result.metrics.max_total_relator_length, 131);
        assert_eq!(result.metrics.max_depth_seen, 6);
        // Pinned from the deterministic run. Orientation sampled the queue
        // after each pop and reported 735,572; the frozen definition tracks
        // the true peak, which is exactly one higher.
        assert_eq!(result.metrics.max_frontier, 735_573);
    }
}

/// Scale regression (ex-oracle): the 4.0M-state fixed workload pins the full
/// v1 row through the C2 implementation. Release-only.
#[cfg(test)]
mod scale_tests {
    use super::*;

    #[test]
    #[ignore = "large: 6.3M moves, ~4.0M states"]
    fn fixed_workload_matches_prototype_scale() {
        let dir =
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../vendor/sair");
        let item = ac_core::data::load_training(dir.join("examples/training_424.json"))
            .expect("training data")
            .into_iter()
            .find(|x| x.training_id == "ms-train-0004")
            .expect("ms-train-0004");
        let limits = SearchLimits {
            max_expanded: None,
            max_depth: None,
            max_total_relator_length: Some(10_000),
            max_work: Some(6_300_000),
        };
        let result = solve_exact_state_bfs(&item.presentation(), &limits);
        assert!(matches!(result.status, SearchStatus::LimitReached));
        assert_eq!(result.certificate_length, None);
        assert!(!result.metrics.replay_ok);
        assert_eq!(result.metrics.expanded, 450_000);
        assert_eq!(result.metrics.generated, 6_300_000);
        assert_eq!(result.metrics.unique, 4_001_727);
        assert_eq!(result.metrics.duplicates, 2_298_274);
        assert_eq!(result.metrics.max_frontier, 3_551_727);
        assert_eq!(result.metrics.max_depth_seen, 7);
        assert_eq!(result.metrics.max_total_relator_length, 416);
    }
}
