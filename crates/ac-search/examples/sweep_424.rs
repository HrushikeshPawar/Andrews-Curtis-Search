//! Uniform-budget sweep of the official 424 training instances through the
//! exact-state BFS. Local benchmark plumbing only: the solver is untouched.
//!
//! Usage: `sweep_424 [shard shards]` (default 0 1 = all cases). Each shard
//! writes one JSON record per case into `results/` and prints a summary.
use ac_core::data::load_training;
use ac_search::{SearchLimits, SearchStatus, solve_exact_state_bfs};
use serde::Serialize;
use std::path::PathBuf;

#[derive(Serialize)]
struct CaseRecord {
    training_id: String,
    official_length: usize,
    status: String,
    error: Option<String>,
    certificate_length: Option<usize>,
    replay_ok: bool,
    certificate: Vec<u8>,
    expanded: u64,
    generated: u64,
    unique: u64,
    duplicates: u64,
    max_frontier: u64,
    max_depth_seen: u32,
    max_total_relator_length: usize,
    elapsed_ms: u128,
}

#[derive(Serialize)]
struct Report {
    format_version: u32,
    algorithm: &'static str,
    limits: SearchLimits,
    shard: usize,
    shards: usize,
    cases: Vec<CaseRecord>,
}

fn main() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    let shard = args.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    let shards = args.next().and_then(|s| s.parse().ok()).unwrap_or(1).max(1);
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../vendor/sair");
    let instances = load_training(root.join("examples/training_424.json"))?;
    let selected: Vec<_> = instances
        .into_iter()
        .enumerate()
        .filter(|(i, _)| i % shards == shard)
        .map(|(_, item)| item)
        .collect();
    // Uniform budget for every case: generous enough to fully cover depth 6,
    // bounded so peak memory stays ~1.2 GB per shard.
    let limits = SearchLimits {
        max_expanded: Some(450_000),
        max_depth: None,
        max_total_relator_length: Some(10_000),
        max_work: Some(6_300_000),
    };
    let mut records = Vec::with_capacity(selected.len());
    let t0 = std::time::Instant::now();
    for item in &selected {
        let result = solve_exact_state_bfs(&item.presentation(), &limits);
        let (status, error) = match &result.status {
            SearchStatus::Solved => ("solved", None),
            SearchStatus::Exhausted => ("exhausted", None),
            SearchStatus::LimitReached => ("limit", None),
            SearchStatus::Unimplemented => ("unimplemented", None),
            SearchStatus::Failed(msg) => ("failed", Some(msg.clone())),
        };
        println!(
            "{} official_len={} {} depth={:?} expanded={} unique={} ms={}",
            item.training_id,
            item.length,
            status,
            result.certificate_length,
            result.metrics.expanded,
            result.metrics.unique,
            result.metrics.elapsed.as_millis(),
        );
        records.push(CaseRecord {
            training_id: item.training_id.clone(),
            official_length: item.length,
            status: status.to_string(),
            error,
            certificate_length: result.certificate_length,
            replay_ok: result.metrics.replay_ok,
            certificate: result.certificate.clone(),
            expanded: result.metrics.expanded,
            generated: result.metrics.generated,
            unique: result.metrics.unique,
            duplicates: result.metrics.duplicates,
            max_frontier: result.metrics.max_frontier,
            max_depth_seen: result.metrics.max_depth_seen,
            max_total_relator_length: result.metrics.max_total_relator_length,
            elapsed_ms: result.metrics.elapsed.as_millis(),
        });
    }
    let solved = records.iter().filter(|r| r.status == "solved").count();
    let report = Report {
        format_version: 1,
        algorithm: "exact_state_bfs",
        limits,
        shard,
        shards,
        cases: records,
    };
    let out = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../results");
    std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
    let name = if shards > 1 {
        format!("exact_bfs_sweep_shard_{shard}.json")
    } else {
        "exact_bfs_sweep.json".to_string()
    };
    std::fs::write(
        out.join(name),
        serde_json::to_string_pretty(&report).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    println!(
        "shard summary: {}/{} solved, wall {}ms",
        solved,
        selected.len(),
        t0.elapsed().as_millis()
    );
    Ok(())
}
