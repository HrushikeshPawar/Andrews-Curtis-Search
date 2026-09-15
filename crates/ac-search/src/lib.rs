//! Search infrastructure only. Exact-state BFS is intentionally absent.
use ac_core::{Certificate, Presentation};
use serde::{Deserialize, Serialize};
use std::time::Duration;

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
/// Reserved seam for the next milestone. Deliberately contains no search loop.
pub fn solve_exact_state_bfs(_start: &Presentation, _limits: &SearchLimits) -> SearchResult {
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
