//! S3 portfolio sweep: every solver × every budget tier over the official
//! training instances or the scored competition corpus. Benchmark plumbing.
//!
//! Usage: `s3_sweep [shard shards solver budget source tag idfile]`
//!   solver: bfs | greedy | beam4 | beam16 | beam64 | atlas | atlas2 | subst | gssub
//!   budget: small | medium | large                   (default: large)
//!   source: train (vendored 424) | scored (SAIR_ACC_ROOT manifest, 10,115)
//!   idfile: optional JSON array of challenge ids to restrict the run
//! Each shard writes CaseReport records (+ certificates) into `results/`.
//! Per-case JSONL and solved-certificate sidecars are flushed as they
//! complete so a killed shard still keeps finished work.
use ac_core::Presentation;
use ac_core::data::{TrainingInstance, load_manifest, load_training};
use ac_search::{
    Budget, CaseReport, GoalBank, ReverseBall, SearchLimits, SearchResult, SearchStatus,
    SubstitutionPolicy, SupermovePolicy, load_submission, solve_bank_greedy, solve_beam,
    solve_exact_state_bfs, solve_length_greedy, solve_substitution_greedy, solve_supermove_greedy,
};
use serde::Serialize;
use std::io::Write;
use std::path::PathBuf;

#[derive(Serialize)]
struct Row {
    training_id: String,
    official_length: Option<usize>,
    start_length: usize,
    via_bank: bool,
    via_ball: bool,
    ball_hit_depth: Option<u32>,
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
            let manifest =
                PathBuf::from(root).join("competition/tools/verifier/data/manifest.json");
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
            let items: Vec<TrainingInstance> =
                load_training(root.join("examples/training_424.json"))?;
            items
                .into_iter()
                .filter(|item| {
                    wanted
                        .as_ref()
                        .is_none_or(|w| w.contains(&item.training_id))
                })
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
    let bank: Option<GoalBank> = if solver.as_str() == "atlas"
        || solver.as_str() == "atlas2"
        || solver.as_str() == "subst"
        || solver.as_str() == "gssub"
    {
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
    // S4.1c: the reverse ball (depth 7, ~2.85M states) is built once per
    // process for the combined solver; measured before committing to 7.
    let mut ball: Option<ReverseBall> = if solver.as_str() == "atlas2" {
        let ball = ReverseBall::build(7);
        eprintln!(
            "ball built: {} states at depth {}",
            ball.len(),
            ball.max_depth()
        );
        Some(ball)
    } else {
        None
    };
    // S4.2b: supermove policy, overridable per run for the experiment matrix.
    let policy = SupermovePolicy {
        max_conjugator: std::env::var("SUBST_MAX_CONJ")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(1),
        top_k: std::env::var("SUBST_TOP_K")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(8),
        keep_increasing: std::env::var("SUBST_KEEP_INC")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(2),
    };
    if solver.as_str() == "subst" {
        eprintln!("supermove policy: {policy:?}");
    }
    let gssub_policy = SubstitutionPolicy {
        top_k: std::env::var("GSSUB_TOP_K")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(16),
    };
    if solver.as_str() == "gssub" {
        eprintln!("gssub policy: {gssub_policy:?}");
    }
    let mut solve = |start: &Presentation| -> (SearchResult, bool, bool, Option<u32>) {
        // Fresh solvers report no bank/ball provenance.
        let no_probe = |result: SearchResult| (result, false, false, None);
        if solver.as_str() == "atlas" {
            let outcome =
                solve_bank_greedy(start, &limits, bank.as_ref().expect("bank built"), None);
            let via = outcome.via_bank;
            (outcome.result, via, false, None)
        } else if solver.as_str() == "atlas2" {
            let outcome = solve_bank_greedy(
                start,
                &limits,
                bank.as_ref().expect("bank built"),
                Some(ball.as_mut().expect("ball built")),
            );
            let (via_bank, via_ball, depth) =
                (outcome.via_bank, outcome.via_ball, outcome.ball_hit_depth);
            (outcome.result, via_bank, via_ball, depth)
        } else if solver.as_str() == "subst" {
            let outcome = solve_supermove_greedy(
                start,
                &limits,
                &policy,
                bank.as_ref().expect("bank built"),
                None,
            );
            (outcome.result, outcome.via_bank, false, None)
        } else if solver.as_str() == "gssub" {
            let outcome = solve_substitution_greedy(
                start,
                &limits,
                &gssub_policy,
                bank.as_ref().expect("bank built"),
            );
            (outcome.result, outcome.via_bank, false, None)
        } else {
            let result = match solver.as_str() {
                "bfs" => solve_exact_state_bfs(start, &limits),
                "greedy" => solve_length_greedy(start, &limits),
                "beam4" => solve_beam(start, &limits, 4),
                "beam16" => solve_beam(start, &limits, 16),
                "beam64" => solve_beam(start, &limits, 64),
                other => panic!("unknown solver {other}"),
            };
            no_probe(result)
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
    let out = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../results");
    std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
    let stem = if shards > 1 {
        format!(
            "s3_{tag}_{source}_{}_{}_shard_{shard}",
            solver_name(&solver),
            budget.name()
        )
    } else {
        format!(
            "s3_{tag}_{source}_{}_{}",
            solver_name(&solver),
            budget.name()
        )
    };
    let mut jsonl = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(out.join(format!("{stem}.jsonl")))
        .map_err(|e| e.to_string())?;
    let mut live_certs = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(out.join(format!("{stem}.certs.txt")))
        .map_err(|e| e.to_string())?;
    for item in &selected {
        let (result, via_bank, via_ball, ball_hit_depth) = solve(&item.start);
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
            via_ball,
            ball_hit_depth,
            report: CaseReport::from_result(solver_name(&solver), budget, item.id.clone(), &result),
            certificate: result.certificate.clone(),
        });
        let row = rows.last().expect("just pushed");
        writeln!(
            jsonl,
            "{}",
            serde_json::to_string(row).map_err(|e| e.to_string())?
        )
        .map_err(|e| e.to_string())?;
        jsonl.flush().map_err(|e| e.to_string())?;
        if row.report.solved {
            writeln!(
                live_certs,
                "{}: {}",
                row.training_id,
                serde_json::to_string(&row.certificate).map_err(|e| e.to_string())?
            )
            .map_err(|e| e.to_string())?;
            live_certs.flush().map_err(|e| e.to_string())?;
        }
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
    let name = format!("{stem}.json");
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
