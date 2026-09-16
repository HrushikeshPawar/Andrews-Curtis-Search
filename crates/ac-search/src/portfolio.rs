//! S3 portfolio plumbing: equal-compute budgets and comparison records.
//!
//! No search semantics live here. Every solver (BFS, greedy, beam) reports
//! through the shared `SearchResult` schema; this module maps those results
//! into per-case comparison records keyed by training id. Ground-truth
//! comparison (`best_known_length`, slack) is filled by the analysis pass,
//! never by the solver.

use super::{SearchResult, SearchStatus};
use serde::{Deserialize, Serialize};

/// Equal-compute budgets, denominated in generated primitive successors —
/// algorithm-independent and already counted by every solver.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Budget {
    Small,
    Medium,
    Large,
}

impl Budget {
    pub fn max_generated(&self) -> u64 {
        match self {
            Budget::Small => 100_000,
            Budget::Medium => 1_000_000,
            Budget::Large => 6_300_000,
        }
    }
    pub fn name(&self) -> &'static str {
        match self {
            Budget::Small => "small",
            Budget::Medium => "medium",
            Budget::Large => "large",
        }
    }
}

/// One solver × one case × one budget. Solver-side fields come from
/// `from_result`; analysis-side fields (`best_known_length`, slack) are
/// attached later by `with_ground_truth`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaseReport {
    pub solver: String,
    pub budget: Budget,
    pub training_id: String,
    pub solved: bool,
    pub certificate_length: Option<usize>,
    pub best_known_length: Option<usize>,
    pub slack_vs_bfs_optimum: Option<i64>,
    pub generated: u64,
    pub expanded: u64,
    pub unique: u64,
    pub peak_frontier: u64,
    pub surviving_width_per_depth: Option<Vec<u32>>,
    pub elapsed_ms: u128,
}

impl CaseReport {
    pub fn from_result(
        solver: impl Into<String>,
        budget: Budget,
        training_id: impl Into<String>,
        result: &SearchResult,
    ) -> Self {
        Self {
            solver: solver.into(),
            budget,
            training_id: training_id.into(),
            solved: matches!(result.status, SearchStatus::Solved),
            certificate_length: result.certificate_length,
            best_known_length: None,
            slack_vs_bfs_optimum: None,
            generated: result.metrics.generated,
            expanded: result.metrics.expanded,
            unique: result.metrics.unique,
            peak_frontier: result.metrics.max_frontier,
            surviving_width_per_depth: result.metrics.surviving_width_per_depth.clone(),
            elapsed_ms: result.metrics.elapsed.as_millis(),
        }
    }

    /// Attach BFS ground truth. Slack is defined only when this report
    /// solved and an optimum is known.
    pub fn with_ground_truth(mut self, best_known_length: usize) -> Self {
        self.best_known_length = Some(best_known_length);
        self.slack_vs_bfs_optimum = self
            .certificate_length
            .map(|len| len as i64 - best_known_length as i64);
        self
    }
}

#[cfg(test)]
mod portfolio_tests {
    use super::*;
    use crate::{SearchLimits, SearchMetrics};

    fn solved_result(cert_len: usize, generated: u64) -> SearchResult {
        SearchResult {
            status: SearchStatus::Solved,
            certificate: vec![0; cert_len],
            certificate_length: Some(cert_len),
            metrics: SearchMetrics {
                generated,
                expanded: 7,
                unique: 42,
                max_frontier: 9,
                ..SearchMetrics::default()
            },
        }
    }

    #[test]
    fn budgets_match_the_frozen_tiers() {
        assert_eq!(Budget::Small.max_generated(), 100_000);
        assert_eq!(Budget::Medium.max_generated(), 1_000_000);
        assert_eq!(Budget::Large.max_generated(), 6_300_000);
    }

    #[test]
    fn report_maps_result_fields_and_computes_slack() {
        let report = CaseReport::from_result(
            "greedy",
            Budget::Medium,
            "ms-train-0001",
            &solved_result(9, 1_000),
        )
        .with_ground_truth(5);
        assert!(report.solved);
        assert_eq!(report.certificate_length, Some(9));
        assert_eq!(report.best_known_length, Some(5));
        assert_eq!(report.slack_vs_bfs_optimum, Some(4));
        assert_eq!(report.generated, 1_000);
        assert_eq!(report.surviving_width_per_depth, None);
        let json = serde_json::to_string(&report).unwrap();
        let back: CaseReport = serde_json::from_str(&json).unwrap();
        assert_eq!(back.slack_vs_bfs_optimum, Some(4));
    }

    #[test]
    fn slack_is_absent_without_a_solution() {
        let mut result = solved_result(9, 1_000);
        result.status = SearchStatus::LimitReached;
        result.certificate_length = None;
        let report =
            CaseReport::from_result("beam-16", Budget::Large, "ms-train-0004", &result)
                .with_ground_truth(7);
        assert!(!report.solved);
        assert_eq!(report.best_known_length, Some(7));
        assert_eq!(report.slack_vs_bfs_optimum, None);
    }

    #[test]
    fn search_limits_express_a_budget_tier() {
        // The harness runs every solver with max_work set to the tier cap;
        // nothing solver-specific may smuggle in extra work.
        let limits = SearchLimits {
            max_expanded: None,
            max_depth: None,
            max_total_relator_length: Some(10_000),
            max_work: Some(Budget::Small.max_generated()),
        };
        assert_eq!(limits.max_work, Some(100_000));
    }
}
