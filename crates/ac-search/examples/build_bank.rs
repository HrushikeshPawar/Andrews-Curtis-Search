//! S4.1 bank builder (throwaway-adjacent plumbing): replay every verified
//! certificate once and record build statistics. The bank itself is a build
//! artifact (regenerable from committed inputs); this binary reports its
//! size so we can decide whether to persist it.
//!
//! Usage: `build_bank [out.json]` (default: /tmp/opencode/sweep/goalbank_v1.json).
//! Needs SAIR_ACC_ROOT for the scored starts.
use ac_core::data::{load_manifest, load_training};
use ac_core::Presentation;
use ac_search::{load_submission, GoalBank};
use std::collections::HashMap;
use std::path::PathBuf;

fn main() -> Result<(), String> {
    let _out = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "/tmp/opencode/sweep/goalbank_v1.json".to_string());
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut starts: HashMap<String, Presentation> = HashMap::new();
    for item in load_training(repo.join("vendor/sair/examples/training_424.json"))? {
        starts.insert(item.training_id.clone(), item.presentation());
    }
    let sair = std::env::var("SAIR_ACC_ROOT")
        .map_err(|_| "SAIR_ACC_ROOT must point to the official SAIR checkout".to_string())?;
    for c in load_manifest(
        PathBuf::from(sair).join("competition/tools/verifier/data/manifest.json"),
    )? {
        starts.insert(c.challenge_id.clone(), c.presentation());
    }
    let mut bank = GoalBank::new();
    let mut lines = 0usize;
    let mut moves_total = 0usize;
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
            moves_total += cert.len();
            bank.insert_certificate(start, &cert)?;
            lines += 1;
        }
    }
    let mut longest = 0usize;
    // Suffix-length distribution in coarse bins.
    let mut bins = vec![0usize; 6];
    for len in bank.suffix_lengths() {
        longest = longest.max(len);
        let b = if len == 0 {
            0
        } else if len < 10 {
            1
        } else if len < 50 {
            2
        } else if len < 200 {
            3
        } else if len < 1000 {
            4
        } else {
            5
        };
        bins[b] += 1;
    }
    println!("certificates replayed: {lines}");
    println!("certificate moves total: {moves_total}");
    println!("distinct banked states: {}", bank.len());
    println!("longest suffix: {longest}");
    println!("suffix bins [0, <10, <50, <200, <1000, >=1000]: {bins:?}");
    Ok(())
}
