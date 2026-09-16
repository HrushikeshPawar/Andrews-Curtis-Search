//! S3 portfolio sweep: every solver × every budget tier over the official
//! 424 training instances. Local benchmark plumbing only.
//!
//! Usage: `s3_sweep [shard shards solver budget max_official tag]`
//!   solver: bfs | greedy | beam4 | beam16 | beam64   (default: greedy)
//!   budget: small | medium | large                   (default: large)
//! Each shard writes CaseReport records (+ certificates) into `results/`.
use ac_core::data::load_training;
use ac_search::{Budget, CaseReport, SearchLimits, SearchStatus};
use ac_search::{solve_beam, solve_exact_state_bfs, solve_length_greedy};
use serde::Serialize;
use std::path::PathBuf;

#[derive(Serialize)]
struct Row {
    training_id: String,
    official_length: usize,
    #[serde(flatten)]
    report: CaseReport,
    certificate: Vec<u8>,
}

#[derive(Serialize)]
struct Report {
    format_version: u32,
    solver: String,
    budget: Budget,
    limits: SearchLimits,
    shard: usize,
    shards: usize,
    rows: Vec<Row>,
}

fn parse_budget(s: Option<String>) -> Budget {
    match s.as_deref() {
        Some("small") => Budget::Small,
        Some("medium") => Budget::Medium,
        _ => Budget::Large,
    }
}

fn main() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    let shard = args.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    let shards = args.next().and_then(|s| s.parse().ok()).unwrap_or(1).max(1);
    let solver = args.next().unwrap_or_else(|| "greedy".to_string());
    let budget = parse_budget(args.next());
    let max_official = args.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    let tag = args.next().unwrap_or_else(|| "s3".to_string());
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../vendor/sair");
    let instances = load_training(root.join("examples/training_424.json"))?;
    let selected: Vec<_> = instances
        .into_iter()
        .enumerate()
        .filter(|(i, _)| i % shards == shard)
        .filter(|(_, item)| max_official == 0 || item.length <= max_official)
        .map(|(_, item)| item)
        .collect();
    let limits = SearchLimits {
        max_expanded: None,
        max_depth: None,
        max_total_relator_length: Some(10_000),
        max_work: Some(budget.max_generated()),
    };
    let solve = |item: &ac_core::data::TrainingInstance| match solver.as_str() {
        "bfs" => solve_exact_state_bfs(&item.presentation(), &limits),
        "greedy" => solve_length_greedy(&item.presentation(), &limits),
        "beam4" => solve_beam(&item.presentation(), &limits, 4),
        "beam16" => solve_beam(&item.presentation(), &limits, 16),
        "beam64" => solve_beam(&item.presentation(), &limits, 64),
        other => panic!("unknown solver {other}"),
    };
fn solver_name(s: &str) -> &str {
    match s {
        "beam4" => "beam-4",
        "beam16" => "beam-16",
        "beam64" => "beam-64",
        other => other,
    }
}
    let mut rows = Vec::with_capacity(selected.len());
    let t0 = std::time::Instant::now();
    for item in &selected {
        let result = solve(item);
        let status = match &result.status {
            SearchStatus::Solved => "solved",
            SearchStatus::Exhausted => "exhausted",
            SearchStatus::LimitReached => "limit",
            SearchStatus::Unimplemented => "unimplemented",
            SearchStatus::Failed(msg) => {
                return Err(format!("{} failed: {msg}", item.training_id));
            }
        };
        println!(
            "{} official_len={} {} cert_len={:?} gen={} uniq={} ms={}",
            item.training_id,
            item.length,
            status,
            result.certificate_length,
            result.metrics.generated,
            result.metrics.unique,
            result.metrics.elapsed.as_millis(),
        );
        rows.push(Row {
            training_id: item.training_id.clone(),
            official_length: item.length,
            report: CaseReport::from_result(
                solver_name(&solver),
                budget,
                item.training_id.clone(),
                &result,
            ),
            certificate: result.certificate.clone(),
        });
    }
    let solved = rows.iter().filter(|r| r.report.solved).count();
    let report = Report {
        format_version: 1,
        solver: solver_name(&solver).to_string(),
        budget,
        limits,
        shard,
        shards,
        rows,
    };
    let out = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../results");
    std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
    let name = if shards > 1 {
        format!(
            "s3_{tag}_{}_{}_shard_{shard}.json",
            solver_name(&solver),
            budget.name()
        )
    } else {
        format!("s3_{tag}_{}_{}.json", solver_name(&solver), budget.name())
    };
    std::fs::write(
        out.join(name),
        serde_json::to_string_pretty(&report).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    println!(
        "shard summary: {solver}/{}: {solved}/{} solved, wall {}ms",
        budget.name(),
        selected.len(),
        t0.elapsed().as_millis()
    );
    Ok(())
}
