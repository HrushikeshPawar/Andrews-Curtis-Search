//! Targeted Q1 quality-mode GSSUB shortening.
//!
//! Usage:
//! `cargo run --release -p ac-search --example q1_quality_gssub -- STARTS_JSON OUT_DIR BASELINE_JSONL [--max-old-length=N]`
//!
//! The baseline JSONL is the verified Q1 candidate inventory. Its certificates
//! seed the shared exact bank, and each case's current candidate length is the
//! incumbent that quality mode must beat.

use ac_core::{Presentation, replay_certificate};
use ac_search::{
    GoalBank, ScoredCertificate, SearchLimits, SearchStatus, StartCatalog, SubstitutionPolicy,
    load_result_jsonl, solve_substitution_quality,
};
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
    old_length: usize,
    incumbent_length: usize,
    new_length: Option<usize>,
    shortened: bool,
    replay_ok: bool,
    method: &'static str,
    certificate: Vec<u8>,
    expanded: u64,
    generated: u64,
    elapsed_ms: u128,
}

fn read_baselines(path: &Path, catalog: &StartCatalog) -> Result<Vec<Case>, String> {
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
        if row.certificate.len() != new_length {
            return Err(format!(
                "{}: {}: certificate length {} != new_length {}",
                path.display(),
                row.challenge_id,
                row.certificate.len(),
                new_length
            ));
        }
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

fn load_completed(path: &Path) -> Result<HashSet<String>, String> {
    if !path.exists() {
        return Ok(HashSet::new());
    }
    let file = File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut completed = HashSet::new();
    for line in BufReader::new(file).lines() {
        let line = line.map_err(|e| format!("{}: {e}", path.display()))?;
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(&line)
            && let Some(id) = value.get("challenge_id").and_then(|value| value.as_str())
        {
            completed.insert(id.to_string());
        }
    }
    Ok(completed)
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
            .ok_or("usage: q1_quality_gssub STARTS_JSON OUT_DIR BASELINE_JSONL")?,
    );
    let out_dir = PathBuf::from(
        args.next()
            .ok_or("usage: q1_quality_gssub STARTS_JSON OUT_DIR BASELINE_JSONL")?,
    );
    let baseline_path = PathBuf::from(
        args.next()
            .ok_or("usage: q1_quality_gssub STARTS_JSON OUT_DIR BASELINE_JSONL")?,
    );
    let max_old_length = args.find_map(|arg| {
        arg.to_str()
            .and_then(|value| value.strip_prefix("--max-old-length="))
            .and_then(|value| value.parse::<usize>().ok())
    });

    fs::create_dir_all(&out_dir).map_err(|e| format!("{}: {e}", out_dir.display()))?;
    let catalog = StartCatalog::load(&starts)?;
    let cases = read_baselines(&baseline_path, &catalog)?;
    let output_path = out_dir.join("candidates.jsonl");
    let completed = load_completed(&output_path)?;
    let mut bank = GoalBank::new();
    let bank_records: Vec<ScoredCertificate> = load_result_jsonl(&baseline_path, &catalog)?;
    for record in &bank_records {
        bank.insert_certificate(&record.start, &record.certificate)?;
    }
    eprintln!(
        "quality bank built: {} states; cases: {}",
        bank.len(),
        cases.len()
    );

    let policy = SubstitutionPolicy {
        top_k: std::env::var("GSSUB_TOP_K")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(16),
    };
    let limits = SearchLimits {
        max_expanded: None,
        max_depth: None,
        max_total_relator_length: Some(10_000),
        max_work: Some(
            std::env::var("GSSUB_QUALITY_MAX_WORK")
                .ok()
                .and_then(|value| value.parse().ok())
                .unwrap_or(1_000_000),
        ),
    };
    let output_file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&output_path)
        .map_err(|e| format!("{}: {e}", output_path.display()))?;
    let mut output = BufWriter::new(output_file);
    for case in cases {
        if max_old_length.is_some_and(|limit| case.incumbent.len() > limit) {
            continue;
        }
        if completed.contains(&case.challenge_id) {
            continue;
        }
        let outcome =
            solve_substitution_quality(&case.start, &limits, &policy, &bank, case.incumbent.len());
        let candidate = if matches!(outcome.result.status, SearchStatus::Solved) {
            outcome.result.certificate
        } else {
            case.incumbent.clone()
        };
        let replay_ok = replay_certificate(&case.start, &candidate).is_ok();
        let shortened = replay_ok && candidate.len() < case.incumbent.len();
        let row = OutputRow {
            challenge_id: case.challenge_id,
            old_length: case.incumbent.len(),
            incumbent_length: case.incumbent.len(),
            new_length: replay_ok.then_some(candidate.len()),
            shortened,
            replay_ok,
            method: "quality-gssub",
            certificate: candidate,
            expanded: outcome.result.metrics.expanded,
            generated: outcome.result.metrics.generated,
            elapsed_ms: outcome.result.metrics.elapsed.as_millis(),
        };
        append_row(&mut output, &row)?;
        eprintln!(
            "completed {}: {} -> {}",
            row.challenge_id,
            row.incumbent_length,
            row.new_length.unwrap_or(row.incumbent_length)
        );
    }
    Ok(())
}
