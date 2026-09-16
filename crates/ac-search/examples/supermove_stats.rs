//! Supermove proposal statistics (S4.2b tuning aid).
//!
//! Usage:
//!   `supermove_stats <challenge_id> [...]`   per-state candidate details
//!   `supermove_stats bank:<n>`               sample n banked (atlas) states
//! Needs SAIR_ACC_ROOT. The bank sample answers: do states on known
//! solution corridors have cancellation-producing supermoves at all?

use ac_core::data::{load_manifest, load_training};
use ac_core::{DEFAULT_N_GEN, Move, Presentation, apply_move, move_from_id};
use ac_search::{Supermove, SupermovePolicy, load_submission, propose_all};
use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;

const SUBMISSIONS: [&str; 4] = [
    "results/s3_403_submission.txt",
    "results/bfs_21_submission.txt",
    "results/s31_814_a.txt",
    "results/s31_814_b.txt",
];

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn policy() -> SupermovePolicy {
    SupermovePolicy {
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
    }
}

fn build_bank() -> Result<Vec<Presentation>, String> {
    let root = repo_root();
    let mut starts: HashMap<String, Presentation> = HashMap::new();
    for item in load_training(root.join("vendor/sair/examples/training_424.json"))? {
        starts.insert(item.training_id.clone(), item.presentation());
    }
    let sair = std::env::var("SAIR_ACC_ROOT")
        .map_err(|_| "SAIR_ACC_ROOT must point to the official SAIR checkout".to_string())?;
    let manifest = PathBuf::from(sair).join("competition/tools/verifier/data/manifest.json");
    for c in load_manifest(manifest)? {
        starts.insert(c.challenge_id.clone(), c.presentation());
    }
    let mut states: Vec<Presentation> = Vec::new();
    for name in SUBMISSIONS {
        for (id, cert) in load_submission(&root.join(name))? {
            let mut state = starts
                .get(&id)
                .ok_or_else(|| format!("unknown challenge {id}"))?
                .clone();
            let table: [Move; 14] =
                core::array::from_fn(|i| move_from_id(i as u8).expect("ids 0..=13 are pinned"));
            states.push(state.clone());
            for &m in &cert {
                state = apply_move(&state, &table[m as usize], DEFAULT_N_GEN).map_err(|e| e.0)?;
                states.push(state.clone());
            }
        }
    }
    states.sort_by(|a, b| a.relators.cmp(&b.relators));
    states.dedup();
    Ok(states)
}

fn trace(pres: &Presentation, proposal: &Supermove) -> Vec<usize> {
    let table: [Move; 14] =
        core::array::from_fn(|i| move_from_id(i as u8).expect("ids 0..=13 are pinned"));
    let mut state = pres.clone();
    let mut lengths = vec![state.total_relator_length()];
    for &id in &proposal.witness {
        state = apply_move(&state, &table[id as usize], DEFAULT_N_GEN).expect("official move");
        lengths.push(state.total_relator_length());
    }
    assert_eq!(state, proposal.successor, "trace must end at the successor");
    lengths
}

fn bank_sample(n: usize, policy: &SupermovePolicy) -> Result<(), String> {
    let states = build_bank()?;
    println!(
        "bank: {} distinct states; sampling every {}",
        states.len(),
        states.len() / n.max(1)
    );
    let stride = (states.len() / n).max(1);
    let (mut sampled, mut with_cancel, mut cancelled_total) = (0usize, 0usize, 0i64);
    let mut best_hist: BTreeMap<i64, usize> = BTreeMap::new();
    for state in states.iter().step_by(stride).take(n) {
        let all = propose_all(state, policy);
        let best = all.iter().map(|m| m.cancelled).max().unwrap_or(0);
        *best_hist.entry(best).or_default() += 1;
        if best > 0 {
            with_cancel += 1;
        }
        cancelled_total += all.iter().map(|m| m.cancelled.max(0)).sum::<i64>();
        sampled += 1;
    }
    println!(
        "sample: {} states | with cancelling proposal: {} ({:.1}%) | cancelled letters: {} ({:.2}/state)",
        sampled,
        with_cancel,
        100.0 * with_cancel as f64 / sampled.max(1) as f64,
        cancelled_total,
        cancelled_total as f64 / sampled.max(1) as f64,
    );
    println!("best-cancellation histogram: {best_hist:?}");
    Ok(())
}

fn main() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        return Err("usage: supermove_stats <challenge_id> [...] | bank:<n>".to_string());
    }
    let policy = policy();
    println!("policy: {policy:?}");
    if let Some(n) = args[0].strip_prefix("bank:") {
        return bank_sample(n.parse().expect("bank:<n>"), &policy);
    }
    let sair = std::env::var("SAIR_ACC_ROOT")
        .map_err(|_| "SAIR_ACC_ROOT must point to the official SAIR checkout".to_string())?;
    let manifest = PathBuf::from(sair).join("competition/tools/verifier/data/manifest.json");
    let challenges = load_manifest(manifest)?;
    for id in &args {
        let challenge = challenges
            .iter()
            .find(|c| &c.challenge_id == id)
            .ok_or_else(|| format!("unknown challenge {id}"))?;
        let pres = challenge.presentation();
        let all = propose_all(&pres, &policy);
        let mut hist: BTreeMap<i64, usize> = BTreeMap::new();
        for m in &all {
            *hist.entry(m.cancelled).or_default() += 1;
        }
        println!(
            "{id}: len={} candidates={} cancelling={} hist={:?}",
            pres.total_relator_length(),
            all.len(),
            all.iter().filter(|m| m.cancelled > 0).count(),
            hist,
        );
        for m in all.iter().take(3) {
            println!(
                "    cancelled={} witness={:?} lengths={:?}",
                m.cancelled,
                m.witness,
                trace(&pres, m)
            );
        }
    }
    Ok(())
}
