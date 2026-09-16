//! S3 portfolio sweep: every solver × every budget tier over the official
//! training instances or the scored competition corpus. Benchmark plumbing.
//!
//! Usage: `s3_sweep [shard shards solver budget source tag idfile]`
//!   solver: bfs | greedy | beam4 | beam16 | beam64   (default: greedy)
//!   budget: small | medium | large                   (default: large)
//!   source: train (vendored 424) | scored (SAIR_ACC_ROOT manifest, 10,115)
//!   idfile: optional JSON array of challenge ids to restrict the run
//! Each shard writes CaseReport records (+ certificates) into `results/`.
use ac_core::data::{TrainingInstance, load_manifest, load_training};
use ac_core::Presentation;
use ac_search::{
    load_submission, solve_bank_greedy, solve_beam, solve_exact_state_bfs, solve_length_greedy,
    Budget, CaseReport, GoalBank, SearchLimits, SearchResult, SearchStatus,
};
use serde::Serialize;
use std::path::PathBuf;

#[derive(Serialize)]
struct Row {
    training_id: String,
    official_length: Option<usize>,
    start_length: usize,
    via_bank: bool,
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
    let source = args.next().unwrap_or_else(|| "train".to_string());
    let tag = args.next().unwrap_or_else(|| "s3".to_string());
    let wanted: Option<std::collections::HashSet<String>> = args
        .next()
        .map(|p| serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap());
    struct Item {
        id: String,
        official: Option<usize>,
        start: Presentation,
    }
    let selected: Vec<Item> = match source.as_str() {
        "scored" => {
            let root = std::env::var("SAIR_ACC_ROOT").map_err(|_| {
                "source=scored needs SAIR_ACC_ROOT pointing at the official SAIR checkout"
                    .to_string()
            })?;
            let manifest = PathBuf::from(root).join("competition/tools/verifier/data/manifest.json");
            load_manifest(&manifest)?
                .into_iter()
                .filter(|c| wanted.as_ref().is_none_or(|w| w.contains(&c.challenge_id)))
                .enumerate()
                .filter(|(i, _)| i % shards == shard)
                .map(|(_, c)| Item {
                    id: c.challenge_id.clone(),
                    official: None,
                    start: c.presentation(),
                })
                .collect()
        }
        _ => {
            let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../vendor/sair");
            let items: Vec<TrainingInstance> = load_training(root.join("examples/training_424.json"))?;
            items
                .into_iter()
                .filter(|item| wanted.as_ref().is_none_or(|w| w.contains(&item.training_id)))
                .enumerate()
                .filter(|(i, _)| i % shards == shard)
                .map(|(_, item)| Item {
                    id: item.training_id.clone(),
                    official: Some(item.length),
                    start: item.presentation(),
                })
                .collect()
        }
    };
    let limits = SearchLimits {
        max_expanded: None,
        max_depth: None,
        max_total_relator_length: Some(10_000),
        max_work: Some(budget.max_generated()),
    };
    // S4.1: the atlas solver needs the goal bank (1238 verified
    // trajectories) built once up front; per-case starts come from the
    // same loaders as `selected` above.
    let bank: Option<GoalBank> = if solver.as_str() == "atlas" {
        use std::collections::HashMap;
        let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        let mut starts: HashMap<String, Presentation> = HashMap::new();
        for item in load_training(repo.join("vendor/sair/examples/training_424.json"))? {
            starts.insert(item.training_id.clone(), item.presentation());
        }
        let sair = std::env::var("SAIR_ACC_ROOT").map_err(|_| {
            "solver=atlas needs SAIR_ACC_ROOT pointing at the official SAIR checkout".to_string()
        })?;
        for c in load_manifest(
            PathBuf::from(sair).join("competition/tools/verifier/data/manifest.json"),
        )? {
            starts.insert(c.challenge_id.clone(), c.presentation());
        }
        let mut bank = GoalBank::new();
        for name in [
            "results/s3_403_submission.txt",
            "results/bfs_21_submission.txt",
            "results/s31_814_a.txt",
            "results/s31_814_b.txt",
        ] {
            for (id, cert) in load_submission(&repo.join(name))? {
                let start = starts
                    .get(&id)
                    .ok_or_else(|| format!("unknown challenge {id}"))?;
                bank.insert_certificate(start, &cert)?;
            }
        }
        eprintln!("bank built: {} states", bank.len());
        Some(bank)
    } else {
        None
    };
    let solve = |start: &Presentation| -> (SearchResult, bool) {
        if solver.as_str() == "atlas" {
            let outcome = solve_bank_greedy(start, &limits, bank.as_ref().expect("bank built"));
            let via = outcome.via_bank;
            (outcome.result, via)
        } else {
            let result = match solver.as_str() {
                "bfs" => solve_exact_state_bfs(start, &limits),
                "greedy" => solve_length_greedy(start, &limits),
                "beam4" => solve_beam(start, &limits, 4),
                "beam16" => solve_beam(start, &limits, 16),
                "beam64" => solve_beam(start, &limits, 64),
                other => panic!("unknown solver {other}"),
            };
            (result, false)
        }
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
        let (result, via_bank) = solve(&item.start);
        let status = match &result.status {
            SearchStatus::Solved => "solved",
            SearchStatus::Exhausted => "exhausted",
            SearchStatus::LimitReached => "limit",
            SearchStatus::Unimplemented => "unimplemented",
            SearchStatus::Failed(msg) => {
                return Err(format!("{} failed: {msg}", item.id));
            }
        };
        println!(
            "{} start_len={} {} cert_len={:?} gen={} uniq={} ms={}",
            item.id,
            item.start.total_relator_length(),
            status,
            result.certificate_length,
            result.metrics.generated,
            result.metrics.unique,
            result.metrics.elapsed.as_millis(),
        );
        rows.push(Row {
            training_id: item.id.clone(),
            official_length: item.official,
            start_length: item.start.total_relator_length(),
            via_bank,
            report: CaseReport::from_result(
                solver_name(&solver),
                budget,
                item.id.clone(),
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
            "s3_{tag}_{source}_{}_{}_shard_{shard}.json",
            solver_name(&solver),
            budget.name()
        )
    } else {
        format!(
            "s3_{tag}_{source}_{}_{}.json",
            solver_name(&solver),
            budget.name()
        )
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
