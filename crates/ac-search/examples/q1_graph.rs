//! Build the global Q1 trajectory graph and run induced closure.
//!
//! Usage:
//! `cargo run --release -p ac-search --example q1_graph -- starts.json out-dir input.jsonl input.txt ...`

use ac_core::replay_certificate;
use ac_search::{
    ScoredCertificate, StartCatalog, TrajectoryGraph, load_result_jsonl,
    load_submission_with_starts,
};
use serde::Serialize;
use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

#[derive(Debug, Serialize)]
struct CandidateRecord {
    challenge_id: String,
    old_length: usize,
    new_length: Option<usize>,
    shortened: bool,
    replay_ok: bool,
    method: &'static str,
    certificate: Vec<u8>,
}

fn load_input(path: &Path, catalog: &StartCatalog) -> Result<Vec<ScoredCertificate>, String> {
    if path.extension().and_then(|ext| ext.to_str()) == Some("txt") {
        load_submission_with_starts(path, catalog)
    } else {
        load_result_jsonl(path, catalog)
    }
}

fn atomic_json(path: &Path, value: &impl Serialize) -> Result<(), String> {
    let tmp = path.with_extension("tmp");
    let file = File::create(&tmp).map_err(|e| format!("{}: {e}", tmp.display()))?;
    serde_json::to_writer_pretty(file, value).map_err(|e| format!("{}: {e}", tmp.display()))?;
    fs::rename(&tmp, path).map_err(|e| format!("{}: {e}", path.display()))
}

fn main() -> Result<(), String> {
    let mut args = std::env::args_os().skip(1);
    let starts = PathBuf::from(
        args.next()
            .ok_or("usage: q1_graph STARTS_JSON OUT_DIR INPUT...")?,
    );
    let out_dir = PathBuf::from(
        args.next()
            .ok_or("usage: q1_graph STARTS_JSON OUT_DIR INPUT...")?,
    );
    let inputs: Vec<_> = args.map(PathBuf::from).collect();
    if inputs.is_empty() {
        return Err("q1_graph requires at least one input file".into());
    }
    fs::create_dir_all(&out_dir).map_err(|e| format!("{}: {e}", out_dir.display()))?;
    let catalog = StartCatalog::load(&starts)?;
    let mut records = HashMap::<String, ScoredCertificate>::new();
    for input in &inputs {
        for record in load_input(input, &catalog)? {
            match records.get(&record.challenge_id) {
                Some(current) if current.old_length <= record.old_length => {}
                _ => {
                    records.insert(record.challenge_id.clone(), record);
                }
            }
        }
    }
    let mut records: Vec<_> = records.into_values().collect();
    records.sort_by(|a, b| a.challenge_id.cmp(&b.challenge_id));

    let mut graph = TrajectoryGraph::new();
    let mut roots = Vec::with_capacity(records.len());
    for record in &records {
        let root =
            graph.ingest_certificate(&record.challenge_id, &record.start, &record.certificate)?;
        roots.push(root);
    }
    let graph_stats = graph.stats();
    let closure_stats = graph.induce_primitive_edges();
    let path_stats = graph.reverse_shortest_paths();

    let candidates_path = out_dir.join("candidates.jsonl");
    let candidates_tmp = out_dir.join("candidates.jsonl.tmp");
    let file = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(&candidates_tmp)
        .map_err(|e| format!("{}: {e}", candidates_tmp.display()))?;
    let mut output = BufWriter::new(file);
    for (record, root) in records.iter().zip(roots) {
        let candidate = graph.shortest_certificate(root);
        let (new_length, shortened, replay_ok, certificate) = match candidate {
            Some(candidate) => {
                let replay_ok = replay_certificate(&record.start, &candidate).is_ok();
                let shortened = replay_ok && candidate.len() < record.old_length;
                (Some(candidate.len()), shortened, replay_ok, candidate)
            }
            None => (None, false, false, Vec::new()),
        };
        serde_json::to_writer(
            &mut output,
            &CandidateRecord {
                challenge_id: record.challenge_id.clone(),
                old_length: record.old_length,
                new_length,
                shortened,
                replay_ok,
                method: "induced-primitive-graph",
                certificate,
            },
        )
        .map_err(|e| e.to_string())?;
        output.write_all(b"\n").map_err(|e| e.to_string())?;
    }
    output.flush().map_err(|e| e.to_string())?;
    fs::rename(&candidates_tmp, &candidates_path)
        .map_err(|e| format!("{}: {e}", candidates_path.display()))?;
    atomic_json(
        &out_dir.join("graph_stats.json"),
        &serde_json::json!({
            "sources": records.len(),
            "graph": graph_stats,
            "closure": closure_stats,
            "paths": path_stats,
        }),
    )?;
    Ok(())
}
