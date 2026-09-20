//! Exact bounded Q1 shortening: prove or search for a certificate below the
//! current incumbent length on short cases.
//!
//! Usage:
//! `cargo run --release -p ac-search --example q1_exact_bound -- STARTS_JSON OUT_DIR BASELINE_JSONL --max-old-length=20`

use ac_core::{Presentation, replay_certificate};
use ac_search::{SearchLimits, SearchStatus, StartCatalog, solve_exact_state_bfs};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize)]
struct BaselineRow {
    challenge_id: String,
    new_length: Option<usize>,
    certificate: Vec<u8>,
}

#[derive(Debug, Clone)]
struct Case {
    challenge_id: String,
    start: Presentation,
    incumbent: Vec<u8>,
}

#[derive(Debug, Serialize)]
struct OutputRow {
    challenge_id: String,
    incumbent_length: usize,
    target_bound: usize,
    status: &'static str,
    new_length: Option<usize>,
    replay_ok: bool,
    certificate: Vec<u8>,
    expanded: u64,
    generated: u64,
    elapsed_ms: u128,
}

fn read_cases(path: &Path, catalog: &StartCatalog) -> Result<Vec<Case>, String> {
    let file = File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut cases = Vec::new();
    for (line_number, line) in BufReader::new(file).lines().enumerate() {
        let line = line.map_err(|e| format!("{}:{}: {e}", path.display(), line_number + 1))?;
        if line.trim().is_empty() {
            continue;
        }
        let row: BaselineRow = serde_json::from_str(&line)
            .map_err(|e| format!("{}:{}: {e}", path.display(), line_number + 1))?;
        let Some(new_length) = row.new_length else {
            continue;
        };
        if row.certificate.len() != new_length {
            return Err(format!(
                "{}: {}: baseline length mismatch",
                path.display(),
                row.challenge_id
            ));
        }
        let start = catalog
            .get(&row.challenge_id)
            .ok_or_else(|| {
                format!(
                    "{}: start not found for {}",
                    path.display(),
                    row.challenge_id
                )
            })?
            .clone();
        if replay_certificate(&start, &row.certificate).is_err() {
            return Err(format!(
                "{}: {}: baseline replay failed",
                path.display(),
                row.challenge_id
            ));
        }
        cases.push(Case {
            challenge_id: row.challenge_id,
            start,
            incumbent: row.certificate,
        });
    }
    cases.sort_by(|a, b| a.challenge_id.cmp(&b.challenge_id));
    Ok(cases)
}

fn completed_ids(path: &Path) -> Result<HashSet<String>, String> {
    if !path.exists() {
        return Ok(HashSet::new());
    }
    let file = File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut ids = HashSet::new();
    for line in BufReader::new(file).lines() {
        let line = line.map_err(|e| format!("{}: {e}", path.display()))?;
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(&line)
            && let Some(id) = value.get("challenge_id").and_then(|value| value.as_str())
        {
            ids.insert(id.to_string());
        }
    }
    Ok(ids)
}

fn append_row(output: &mut BufWriter<File>, row: &OutputRow) -> Result<(), String> {
    serde_json::to_writer(&mut *output, row).map_err(|e| e.to_string())?;
    output.write_all(b"\n").map_err(|e| e.to_string())?;
    output.flush().map_err(|e| e.to_string())
}

fn main() -> Result<(), String> {
    let mut args = std::env::args_os().skip(1);
    let starts = PathBuf::from(
        args.next()
            .ok_or("usage: q1_exact_bound STARTS_JSON OUT_DIR BASELINE_JSONL")?,
    );
    let out_dir = PathBuf::from(
        args.next()
            .ok_or("usage: q1_exact_bound STARTS_JSON OUT_DIR BASELINE_JSONL")?,
    );
    let baseline = PathBuf::from(
        args.next()
            .ok_or("usage: q1_exact_bound STARTS_JSON OUT_DIR BASELINE_JSONL")?,
    );
    let max_old_length = args
        .find_map(|arg| {
            arg.to_str()
                .and_then(|v| v.strip_prefix("--max-old-length="))
                .and_then(|v| v.parse().ok())
        })
        .unwrap_or(20usize);
    let max_work = std::env::var("Q1_EXACT_MAX_WORK")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(1_000_000);

    fs::create_dir_all(&out_dir).map_err(|e| format!("{}: {e}", out_dir.display()))?;
    let catalog = StartCatalog::load(&starts)?;
    let cases = read_cases(&baseline, &catalog)?;
    let output_path = out_dir.join("candidates.jsonl");
    let completed = completed_ids(&output_path)?;
    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&output_path)
        .map_err(|e| format!("{}: {e}", output_path.display()))?;
    let mut output = BufWriter::new(file);

    for case in cases {
        if case.incumbent.len() > max_old_length || completed.contains(&case.challenge_id) {
            continue;
        }
        let target_bound = case.incumbent.len() - 1;
        let limits = SearchLimits {
            max_expanded: None,
            max_depth: Some(target_bound as u32),
            max_total_relator_length: Some(10_000),
            max_work: Some(max_work),
        };
        let result = solve_exact_state_bfs(&case.start, &limits);
        let (status, new_length, certificate, replay_ok) = match result.status {
            SearchStatus::Solved => {
                let replay_ok = replay_certificate(&case.start, &result.certificate).is_ok();
                (
                    "found",
                    Some(result.certificate.len()),
                    result.certificate,
                    replay_ok,
                )
            }
            SearchStatus::Exhausted => ("exhausted", None, Vec::new(), false),
            SearchStatus::LimitReached => ("budget-limited", None, Vec::new(), false),
            _ => ("failed", None, Vec::new(), false),
        };
        let row = OutputRow {
            challenge_id: case.challenge_id,
            incumbent_length: case.incumbent.len(),
            target_bound,
            status,
            new_length,
            replay_ok,
            certificate,
            expanded: result.metrics.expanded,
            generated: result.metrics.generated,
            elapsed_ms: result.metrics.elapsed.as_millis(),
        };
        append_row(&mut output, &row)?;
        eprintln!("completed {}: {}", row.challenge_id, row.status);
    }
    Ok(())
}
