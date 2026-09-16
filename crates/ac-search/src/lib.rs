//! Search infrastructure. Exact-state BFS over the ordinary AC graph.
use ac_core::{Certificate, DEFAULT_N_GEN, Presentation, reduce, replay_certificate};
use serde::{Deserialize, Serialize};
use std::time::Duration;

mod atlas_search;
mod ball;
mod beam;
mod goalbank;
mod greedy;
mod portfolio;
mod search;
mod store;

pub use portfolio::{Budget, CaseReport, load_submission};

pub use atlas_search::AtlasOutcome;
pub use ball::ReverseBall;
pub use goalbank::GoalBank;

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
    /// Beam only: beam size after each layer truncation, index = depth.
    /// `None` for BFS/greedy. Defaulted so older records still decode.
    #[serde(default)]
    pub surviving_width_per_depth: Option<Vec<u32>>,
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
/// Deterministic length-greedy search (see module `greedy` for the frozen
/// v1 policy): best-first on total relator length, permanent exact-state
/// suppression, first-discovery certificates. Not shortest-path guaranteed.
pub fn solve_length_greedy(start: &Presentation, limits: &SearchLimits) -> SearchResult {
    greedy::solve_length_greedy(start, limits)
}
/// Deterministic beam search (see module `beam` for the frozen v1 policy):
/// layered best-first on total relator length with the given width, global
/// exact-state suppression, first-discovery certificates.
pub fn solve_beam(
    start: &Presentation,
    limits: &SearchLimits,
    width: usize,
) -> SearchResult {
    beam::solve_beam(start, limits, width)
}
/// Bank-probing greedy with immediate-inverse pruning (see module
/// `atlas_search`): S4.1 experiment only, not a frozen solver. With `ball`,
/// reverse-ball hits compete with bank hits by shorter completion.
pub fn solve_bank_greedy(
    start: &Presentation,
    limits: &SearchLimits,
    bank: &GoalBank,
    ball: Option<&mut ReverseBall>,
) -> atlas_search::AtlasOutcome {
    atlas_search::solve_bank_greedy(start, limits, bank, ball)
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
mod scale_tests {use super::*;

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

/// S3.0b greedy checkpoint: deterministic length-greedy behavior and its
/// relationship to BFS optima on small fixtures.
#[cfg(test)]
mod greedy_tests {
    use super::*;

    fn pres(r0: &[i8], r1: &[i8]) -> Presentation {
        Presentation {
            relators: [r0.to_vec(), r1.to_vec()],
        }
    }

    /// Greedy certificates are valid but never shorter than the BFS optimum.
    fn check_not_shorter_than_optimum(r0: &[i8], r1: &[i8], bfs_depth: usize) {
        let start = pres(r0, r1);
        let bfs = solve_exact_state_bfs(&start, &SearchLimits::default());
        assert!(matches!(bfs.status, SearchStatus::Solved));
        assert_eq!(bfs.certificate_length, Some(bfs_depth));
        let greedy = solve_length_greedy(&start, &SearchLimits::default());
        assert!(
            matches!(greedy.status, SearchStatus::Solved),
            "greedy failed where BFS depth is {bfs_depth}"
        );
        assert!(greedy.metrics.replay_ok);
        let glen = greedy.certificate_length.expect("solved");
        assert!(
            glen >= bfs_depth,
            "greedy len {glen} beats BFS optimum {bfs_depth}"
        );
        println!(
            "greedy_vs_bfs [{r0:?}],[{r1:?}]: bfs={bfs_depth} greedy={glen} \
             greedy_gen={} bfs_gen={}",
            greedy.metrics.generated, bfs.metrics.generated
        );
    }

    #[test]
    fn greedy_never_beats_bfs_optimum_on_fixtures() {
        check_not_shorter_than_optimum(&[1, 2], &[2], 1);
        check_not_shorter_than_optimum(&[1], &[2, 1], 1);
        check_not_shorter_than_optimum(&[1], &[2, -1], 1);
        check_not_shorter_than_optimum(&[-2, 1, 2], &[1, 2, -1], 2);
        check_not_shorter_than_optimum(&[-1, -2], &[-1, -2, 1], 4);
    }

    #[test]
    fn greedy_ms1y_finds_valid_certificate() {
        // BFS optimum is 6; greedy finds a valid length-8 certificate at a
        // fraction of the work (464 vs 1,303,629 generated). First slack +2.
        let result = solve_length_greedy(
            &pres(&[-1, 2, 1, -2, -2], &[-1, 2]),
            &SearchLimits::default(),
        );
        assert!(matches!(result.status, SearchStatus::Solved));
        assert_eq!(result.certificate, vec![0, 2, 9, 3, 1, 2, 5, 1]);
        assert_eq!(result.certificate_length, Some(8));
        assert!(result.metrics.replay_ok);
        assert_eq!(result.metrics.expanded, 33);
        assert_eq!(result.metrics.generated, 464);
        assert_eq!(result.metrics.unique, 283);
        assert_eq!(result.metrics.duplicates, 182);
        assert_eq!(result.metrics.max_frontier, 248);
        assert_eq!(result.metrics.max_depth_seen, 9);
        assert_eq!(result.metrics.max_total_relator_length, 12);
    }

    #[test]
    fn greedy_ms_train_0001_finds_valid_certificate() {
        // BFS optimum is 5 (official walk is 8); greedy finds length 6 with
        // 114 generated vs BFS's 7,402.
        let dir =
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../vendor/sair");
        let item = ac_core::data::load_training(dir.join("examples/training_424.json"))
            .expect("training data")
            .into_iter()
            .find(|x| x.training_id == "ms-train-0001")
            .expect("ms-train-0001");
        let result = solve_length_greedy(&item.presentation(), &SearchLimits::default());
        assert!(matches!(result.status, SearchStatus::Solved));
        assert_eq!(result.certificate, vec![6, 2, 9, 1, 4, 1]);
        assert_eq!(result.certificate_length, Some(6));
        assert!(result.metrics.replay_ok);
        assert_eq!(result.metrics.expanded, 8);
        assert_eq!(result.metrics.generated, 114);
        assert_eq!(result.metrics.unique, 88);
        assert_eq!(result.metrics.duplicates, 27);
        assert_eq!(result.metrics.max_frontier, 78);
        assert_eq!(result.metrics.max_depth_seen, 6);
        assert_eq!(result.metrics.max_total_relator_length, 12);
    }
}

/// S3.0c beam checkpoint: layers, truncation, widths, determinism.
#[cfg(test)]
mod beam_tests {
    use super::*;

    fn pres(r0: &[i8], r1: &[i8]) -> Presentation {
        Presentation {
            relators: [r0.to_vec(), r1.to_vec()],
        }
    }

    #[test]
    fn beam_widths_pin_on_back_4() {
        // BFS optimum is 4; all three widths find optimal certificates.
        // Narrower beams do strictly less work (128 vs 422 vs 1094).
        let start = pres(&[-1, -2], &[-1, -2, 1]);
        let w4 = solve_beam(&start, &SearchLimits::default(), 4);
        assert!(matches!(w4.status, SearchStatus::Solved));
        assert_eq!(w4.certificate, vec![10, 3, 0, 1]);
        assert_eq!(w4.certificate_length, Some(4));
        assert!(w4.metrics.replay_ok);
        assert_eq!(w4.metrics.expanded, 9);
        assert_eq!(w4.metrics.generated, 128);
        assert_eq!(w4.metrics.unique, 88);
        assert_eq!(w4.metrics.duplicates, 41);
        assert_eq!(w4.metrics.max_frontier, 41);
        assert_eq!(w4.metrics.max_depth_seen, 4);
        assert_eq!(w4.metrics.max_total_relator_length, 8);
        assert_eq!(
            w4.metrics.surviving_width_per_depth,
            Some(vec![1, 4, 4, 4])
        );
        let w16 = solve_beam(&start, &SearchLimits::default(), 16);
        assert!(matches!(w16.status, SearchStatus::Solved));
        assert_eq!(w16.certificate, vec![10, 3, 0, 1]);
        assert_eq!(w16.certificate_length, Some(4));
        assert!(w16.metrics.replay_ok);
        assert_eq!(w16.metrics.expanded, 30);
        assert_eq!(w16.metrics.generated, 422);
        assert_eq!(w16.metrics.unique, 266);
        assert_eq!(w16.metrics.duplicates, 157);
        assert_eq!(w16.metrics.max_frontier, 126);
        assert_eq!(w16.metrics.max_depth_seen, 4);
        assert_eq!(w16.metrics.max_total_relator_length, 12);
        assert_eq!(
            w16.metrics.surviving_width_per_depth,
            Some(vec![1, 13, 16, 16])
        );
        let w64 = solve_beam(&start, &SearchLimits::default(), 64);
        assert!(matches!(w64.status, SearchStatus::Solved));
        assert_eq!(w64.certificate, vec![10, 3, 0, 1]);
        assert_eq!(w64.certificate_length, Some(4));
        assert!(w64.metrics.replay_ok);
        assert_eq!(w64.metrics.expanded, 78);
        assert_eq!(w64.metrics.generated, 1094);
        assert_eq!(w64.metrics.unique, 697);
        assert_eq!(w64.metrics.duplicates, 398);
        assert_eq!(w64.metrics.max_frontier, 556);
        assert_eq!(w64.metrics.max_depth_seen, 4);
        assert_eq!(w64.metrics.max_total_relator_length, 13);
        assert_eq!(
            w64.metrics.surviving_width_per_depth,
            Some(vec![1, 13, 64, 64])
        );
    }

    #[test]
    fn beam_ms1y_finds_optimal_certificate() {
        // BFS optimum is 6; beam-16 matches it with 883 generated vs
        // BFS's 1,303,629.
        let result = solve_beam(
            &pres(&[-1, 2, 1, -2, -2], &[-1, 2]),
            &SearchLimits::default(),
            16,
        );
        assert!(matches!(result.status, SearchStatus::Solved));
        assert_eq!(result.certificate, vec![9, 10, 2, 8, 5, 0]);
        assert_eq!(result.certificate_length, Some(6));
        assert!(result.metrics.replay_ok);
        assert_eq!(result.metrics.expanded, 63);
        assert_eq!(result.metrics.generated, 883);
        assert_eq!(result.metrics.unique, 586);
        assert_eq!(result.metrics.duplicates, 298);
        assert_eq!(result.metrics.max_frontier, 152);
        assert_eq!(result.metrics.max_depth_seen, 6);
        assert_eq!(result.metrics.max_total_relator_length, 19);
        assert_eq!(
            result.metrics.surviving_width_per_depth,
            Some(vec![1, 13, 16, 16, 16, 16])
        );
    }

    #[test]
    fn beam_is_deterministic_across_runs() {
        // Randomized hash layout must not leak into traversal: two runs
        // agree on everything except wall time.
        let start = pres(&[-1, -2], &[-1, -2, 1]);
        let a = solve_beam(&start, &SearchLimits::default(), 16);
        let b = solve_beam(&start, &SearchLimits::default(), 16);
        assert_eq!(a.certificate, b.certificate);
        assert_eq!(a.certificate_length, b.certificate_length);
        assert_eq!(format!("{:?}", a.status), format!("{:?}", b.status));
        assert_eq!(a.metrics.expanded, b.metrics.expanded);
        assert_eq!(a.metrics.generated, b.metrics.generated);
        assert_eq!(a.metrics.unique, b.metrics.unique);
        assert_eq!(a.metrics.duplicates, b.metrics.duplicates);
        assert_eq!(a.metrics.max_frontier, b.metrics.max_frontier);
        assert_eq!(a.metrics.max_depth_seen, b.metrics.max_depth_seen);
        assert_eq!(
            a.metrics.surviving_width_per_depth,
            b.metrics.surviving_width_per_depth
        );
    }
}
/// S4.1 experiment tests: bank probing and inverse pruning.
#[cfg(test)]
mod atlas_tests {
    use super::*;

    fn pres(r0: &[i8], r1: &[i8]) -> Presentation {
        Presentation {
            relators: [r0.to_vec(), r1.to_vec()],
        }
    }

    fn xy_bank() -> GoalBank {
        let mut bank = GoalBank::new();
        bank.insert_certificate(&pres(&[1, 2], &[2]), &[3])
            .expect("fixture cert replays");
        bank
    }

    #[test]
    fn banked_start_returns_suffix_without_expansion() {
        let outcome = solve_bank_greedy(
            &pres(&[1, 2], &[2]),
            &SearchLimits::default(),
            &xy_bank(),
            None,
        );
        assert!(matches!(outcome.result.status, SearchStatus::Solved));
        assert!(outcome.via_bank);
        assert!(!outcome.via_ball);
        assert_eq!(outcome.ball_hit_depth, None);
        assert_eq!(outcome.result.certificate, vec![3]);
        assert_eq!(outcome.result.certificate_length, Some(1));
        assert!(outcome.result.metrics.replay_ok);
        assert_eq!(outcome.result.metrics.expanded, 0);
        assert_eq!(outcome.result.metrics.generated, 0);
        assert_eq!(outcome.result.metrics.unique, 1);
    }

    #[test]
    fn mid_path_bank_hit_completes_the_certificate() {
        // P=[[1,2,2],[2]] reaches banked s0=[[1,2],[2]] via move 3 (4th
        // generated successor of the root expansion): cert [3,3].
        let outcome = solve_bank_greedy(
            &pres(&[1, 2, 2], &[2]),
            &SearchLimits::default(),
            &xy_bank(),
            None,
        );
        assert!(matches!(outcome.result.status, SearchStatus::Solved));
        assert!(outcome.via_bank);
        assert!(!outcome.via_ball);
        assert_eq!(outcome.result.certificate, vec![3, 3]);
        assert_eq!(outcome.result.certificate_length, Some(2));
        assert!(outcome.result.metrics.replay_ok);
        assert_eq!(outcome.result.metrics.expanded, 0);
        assert_eq!(outcome.result.metrics.generated, 4);
        assert_eq!(outcome.result.metrics.unique, 5);
    }

    #[test]
    fn ball_probe_is_live_in_the_integrated_solver() {
        // back_4 with an empty bank and ball-3: the root's move-0 successor
        // sits in the ball at depth 3, so the run returns after a single
        // generated successor with the optimal 4-move certificate. A zero
        // hit-rate anywhere else is measurement, not a dead probe.
        fn pres(r0: &[i8], r1: &[i8]) -> crate::Presentation {
            crate::Presentation {
                relators: [r0.to_vec(), r1.to_vec()],
            }
        }
        let mut ball = crate::ball::ReverseBall::build(3);
        let outcome = crate::solve_bank_greedy(
            &pres(&[-1, -2], &[-1, -2, 1]),
            &crate::SearchLimits::default(),
            &crate::GoalBank::new(),
            Some(&mut ball),
        );
        assert!(matches!(
            outcome.result.status,
            crate::SearchStatus::Solved
        ));
        assert!(!outcome.via_bank);
        assert!(outcome.via_ball);
        assert_eq!(outcome.ball_hit_depth, Some(3));
        assert_eq!(outcome.result.certificate, vec![0, 2, 10, 1]);
        assert_eq!(outcome.result.certificate_length, Some(4));
        assert!(outcome.result.metrics.replay_ok);
        assert_eq!(outcome.result.metrics.generated, 1);
    }

    #[test]
    fn ball_hit_prefers_the_optimal_suffix() {
        // P=[[1,2,2],[2]] with an empty bank and ball-2: the 4th generated
        // successor s0 sits in the ball at depth 1, so cert [3,3] wins via
        // the ball with recorded depth 1.
        let mut ball = ReverseBall::build(2);
        let outcome = solve_bank_greedy(
            &pres(&[1, 2, 2], &[2]),
            &SearchLimits::default(),
            &GoalBank::new(),
            Some(&mut ball),
        );
        assert!(matches!(outcome.result.status, SearchStatus::Solved));
        assert!(!outcome.via_bank);
        assert!(outcome.via_ball);
        assert_eq!(outcome.ball_hit_depth, Some(1));
        assert_eq!(outcome.result.certificate, vec![3, 3]);
        assert_eq!(outcome.result.certificate_length, Some(2));
        assert!(outcome.result.metrics.replay_ok);
        assert_eq!(outcome.result.metrics.generated, 4);
    }

    #[test]
    fn empty_bank_matches_greedy_modulo_pruned_edges() {
        // With nothing banked, inverse pruning removes only certain
        // duplicates: same traversal, fewer generated/dups, and the
        // difference is exactly the pruned edges.
        let start = pres(&[-1, -2], &[-1, -2, 1]);
        let empty = GoalBank::new();
        let a = solve_bank_greedy(&start, &SearchLimits::default(), &empty, None);
        let b = solve_bank_greedy(&start, &SearchLimits::default(), &empty, None);
        let g = solve_length_greedy(&start, &SearchLimits::default());
        assert!(!a.via_bank);
        assert!(!a.via_ball);
        for x in [&a.result, &b.result] {
            assert_eq!(format!("{:?}", x.status), format!("{:?}", g.status));
            assert_eq!(x.certificate, g.certificate);
            assert_eq!(x.certificate_length, g.certificate_length);
            assert_eq!(x.metrics.expanded, g.metrics.expanded);
            assert_eq!(x.metrics.unique, g.metrics.unique);
            assert_eq!(x.metrics.max_frontier, g.metrics.max_frontier);
            assert_eq!(x.metrics.max_depth_seen, g.metrics.max_depth_seen);
            assert!(x.metrics.generated <= g.metrics.generated);
            assert_eq!(
                g.metrics.generated - x.metrics.generated,
                g.metrics.duplicates - x.metrics.duplicates,
                "every pruned edge must have been a duplicate"
            );
        }
    }
}
