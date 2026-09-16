//! S4.2a quotient instrumentation: project the populations we already have
//! -- atlas states, verified trajectory states, residue starts, and sampled
//! states from failed residue searches -- into the class-2 nilpotent and
//! abelian quotients, and report the four diagnostic questions from the S4.2
//! plan (region occupancy, trajectory monotonicity, equal-length separation
//! between productive and dead states, and suffix-length correlation).
//!
//! This is an analysis driver, not a solver: every search it runs is the
//! frozen `solve_bank_greedy` with a discovery observer attached.
//!
//! Usage: `quotient_probe [phase] [p]`
//!   phase: starts | atlas | traj | failed | all   (default: all)
//!   p:     quotient modulus                       (default: 5)
//!
//! Needs SAIR_ACC_ROOT for the scored manifest. Writes one JSON per phase to
//! `results/s4_quotient_p{p}_{phase}.json` and prints a readable summary.

use ac_core::data::{load_manifest, load_training};
use ac_core::{DEFAULT_N_GEN, Presentation, apply_move, move_from_id, reduce};
use ac_search::{
    AbelianQuotient, Class2Quotient, DistanceTable, GoalBank, SearchLimits, SearchStatus,
    load_submission, solve_bank_greedy_observed,
};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::PathBuf;

/// Verified trajectories, exactly the four files the S4.1 bank is built from.
const SUBMISSIONS: [&str; 4] = [
    "results/s3_403_submission.txt",
    "results/bfs_21_submission.txt",
    "results/s31_814_a.txt",
    "results/s31_814_b.txt",
];

/// Frozen S3/S4 medium budget: 1M generated primitive successors per case.
const MEDIUM_WORK: u64 = 1_000_000;
/// Sample every Nth discovered state in the failed-search phase.
const SAMPLE_STRIDE: u64 = 16;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn load_starts() -> Result<HashMap<String, Presentation>, String> {
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
    Ok(starts)
}

fn slice_ids() -> Result<Vec<String>, String> {
    let path = repo_root().join("results/atlas_slice_500.json");
    serde_json::from_str(
        &std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?,
    )
    .map_err(|e| format!("{}: {e}", path.display()))
}

fn submission_entries() -> Result<Vec<(String, Vec<u8>)>, String> {
    let mut out = Vec::new();
    for name in SUBMISSIONS {
        out.extend(load_submission(&repo_root().join(name))?);
    }
    Ok(out)
}

/// Replay every submission certificate once, exactly like the bank builder,
/// so projected states are the ones the atlas holds.
fn build_bank(starts: &HashMap<String, Presentation>) -> Result<GoalBank, String> {
    let mut bank = GoalBank::new();
    for (id, cert) in submission_entries()? {
        let start = starts
            .get(&id)
            .ok_or_else(|| format!("unknown challenge {id}"))?;
        bank.insert_certificate(start, &cert)?;
    }
    Ok(bank)
}

fn write_json(name: &str, value: &Value) -> Result<(), String> {
    let path = repo_root().join("results").join(name);
    std::fs::write(
        &path,
        serde_json::to_string_pretty(value).map_err(|e| e.to_string())?,
    )
    .map_err(|e| format!("{}: {e}", path.display()))?;
    println!("wrote {}", path.display());
    Ok(())
}

fn dist_rows(hist: &BTreeMap<u16, u64>) -> Vec<Value> {
    hist.iter()
        .map(|(&dist, &count)| json!({"dist": dist, "count": count}))
        .collect()
}

fn len_qdist_rows(hist: &BTreeMap<(u16, u16), u64>) -> Vec<Value> {
    hist.iter()
        .map(|(&(total_len, qdist), &count)| {
            json!({"total_len": total_len, "qdist": qdist, "count": count})
        })
        .collect()
}

/// n, min, median, max, mean of a small integer histogram.
fn hist_stats(hist: &BTreeMap<u16, u64>) -> (u64, u16, u16, u16, f64) {
    let n: u64 = hist.values().sum();
    if n == 0 {
        return (0, 0, 0, 0, f64::NAN);
    }
    let min = *hist.keys().next().expect("nonempty");
    let max = *hist.keys().next_back().expect("nonempty");
    let sum: u64 = hist.iter().map(|(&d, &c)| d as u64 * c).sum();
    let mut acc = 0u64;
    let mut median = 0u16;
    for (&d, &c) in hist {
        acc += c;
        if acc * 2 >= n {
            median = d;
            break;
        }
    }
    (n, min, median, max, sum as f64 / n as f64)
}

fn stats_json(hist: &BTreeMap<u16, u64>) -> Value {
    let (n, min, median, max, mean) = hist_stats(hist);
    json!({"n": n, "min": min, "p50": median, "max": max, "mean": mean})
}

fn count_at_most(hist: &BTreeMap<u16, u64>, threshold: u16) -> u64 {
    hist.iter()
        .filter(|(d, _)| **d <= threshold)
        .map(|(_, c)| *c)
        .sum()
}

/// Average ranks (1-based, ties averaged) for Spearman correlation.
fn average_ranks(values: &[u32]) -> Vec<f64> {
    let mut order: Vec<usize> = (0..values.len()).collect();
    order.sort_unstable_by_key(|&i| values[i]);
    let mut ranks = vec![0.0f64; values.len()];
    let mut i = 0;
    while i < order.len() {
        let mut j = i;
        while j + 1 < order.len() && values[order[j + 1]] == values[order[i]] {
            j += 1;
        }
        let avg = (i + j) as f64 / 2.0 + 1.0;
        for &k in &order[i..=j] {
            ranks[k] = avg;
        }
        i = j + 1;
    }
    ranks
}

fn spearman(xs: &[u32], ys: &[u32]) -> f64 {
    assert_eq!(xs.len(), ys.len());
    let n = xs.len();
    if n < 2 {
        return f64::NAN;
    }
    let rx = average_ranks(xs);
    let ry = average_ranks(ys);
    let mean = (n as f64 + 1.0) / 2.0;
    let mut num = 0.0;
    let mut dx = 0.0;
    let mut dy = 0.0;
    for (a, b) in rx.iter().zip(&ry) {
        let da = a - mean;
        let db = b - mean;
        num += da * db;
        dx += da * da;
        dy += db * db;
    }
    num / (dx.sqrt() * dy.sqrt())
}

/// Per-length-bucket mean and median of a `(length, qdist)` histogram.
fn bucket_summary(hist: &BTreeMap<(u16, u16), u64>, width: u16) -> Vec<Value> {
    let mut buckets: BTreeMap<u16, BTreeMap<u16, u64>> = BTreeMap::new();
    for (&(len, dist), &count) in hist {
        *buckets
            .entry(len / width * width)
            .or_default()
            .entry(dist)
            .or_default() += count;
    }
    buckets
        .into_iter()
        .map(|(bucket, qdist_hist)| {
            let (n, _, median, _, mean) = hist_stats(&qdist_hist);
            json!({"bucket": bucket, "n": n, "mean_qdist": mean, "p50_qdist": median})
        })
        .collect()
}

/// Replayed intermediate states of one certificate (start, then after each move).
fn replay_states(start: &Presentation, cert: &[u8]) -> Result<Vec<Presentation>, String> {
    let mut state = Presentation {
        relators: [
            reduce(&start.relators[0], DEFAULT_N_GEN).map_err(|e| e.0)?,
            reduce(&start.relators[1], DEFAULT_N_GEN).map_err(|e| e.0)?,
        ],
    };
    let mut states = Vec::with_capacity(cert.len() + 1);
    states.push(state.clone());
    for &id in cert {
        let mv = move_from_id(id).map_err(|e| e.0)?;
        state = apply_move(&state, &mv, DEFAULT_N_GEN).map_err(|e| e.0)?;
        states.push(state.clone());
    }
    Ok(states)
}

/// One quotient pair plus the precomputed distance tables a phase needs.
struct QuotientCtx {
    c2: Class2Quotient,
    ab: AbelianQuotient,
    c2_target: DistanceTable,
    ab_target: DistanceTable,
}

impl QuotientCtx {
    fn new(p: u8) -> Self {
        Self {
            c2: Class2Quotient::new(p),
            ab: AbelianQuotient::new(p),
            c2_target: Class2Quotient::new(p).distance_from_target(),
            ab_target: AbelianQuotient::new(p).distance_from_target(),
        }
    }

    fn c2_dist(&self, state: &Presentation) -> u16 {
        self.c2_target.get(self.c2.index(&self.c2.project(state)))
    }

    fn ab_dist(&self, state: &Presentation) -> u16 {
        self.ab_target.get(self.ab.index(&self.ab.project(state)))
    }
}

struct PopulationSummary {
    rows: Vec<Value>,
    c2_hist: BTreeMap<u16, u64>,
    ab_hist: BTreeMap<u16, u64>,
    atlas_hist: BTreeMap<u16, u64>,
    len_hist: BTreeMap<u16, u64>,
    atlas_states: u64,
    atlas_suffix_total: u64,
}

fn summarize_population(
    ctx: &QuotientCtx,
    starts: &HashMap<String, Presentation>,
    bank: &GoalBank,
    atlas_table: &DistanceTable,
    ids: &[String],
) -> Result<PopulationSummary, String> {
    let mut out = PopulationSummary {
        rows: Vec::with_capacity(ids.len()),
        c2_hist: BTreeMap::new(),
        ab_hist: BTreeMap::new(),
        atlas_hist: BTreeMap::new(),
        len_hist: BTreeMap::new(),
        atlas_states: 0,
        atlas_suffix_total: 0,
    };
    for id in ids {
        let start = starts
            .get(id)
            .ok_or_else(|| format!("unknown challenge {id}"))?;
        let sig = ctx.c2.project(start);
        let c2_dist = ctx.c2_target.get(ctx.c2.index(&sig));
        let ab_dist = ctx.ab_dist(start);
        let suffix = bank.suffix(start).map(<[u8]>::len);
        let to_atlas = atlas_table.get(ctx.c2.index(&sig));
        *out.c2_hist.entry(c2_dist).or_default() += 1;
        *out.ab_hist.entry(ab_dist).or_default() += 1;
        *out.atlas_hist.entry(to_atlas).or_default() += 1;
        *out.len_hist
            .entry(start.total_relator_length() as u16)
            .or_default() += 1;
        if let Some(len) = suffix {
            out.atlas_states += 1;
            out.atlas_suffix_total += len as u64;
        }
        out.rows.push(json!({
            "id": id,
            "start_len": start.total_relator_length(),
            "c2_dist": c2_dist,
            "ab_dist": ab_dist,
            "atlas_state": suffix.is_some(),
            "atlas_suffix": suffix,
            "dist_to_atlas_sig": to_atlas,
        }));
    }
    Ok(out)
}

/// Abstract-distance histograms of the residue starts and of every solved
/// start, plus distance to the nearest atlas signature.
///
/// Post-mortem for the failed ball experiment: a state inside reverse-ball
/// depth `d` must have abstract distance `<= d`, so the fraction of starts
/// with `c2_dist <= d` bounds the fraction the ball could ever terminate.
fn phase_starts(
    p: u8,
    starts: &HashMap<String, Presentation>,
    bank: &GoalBank,
) -> Result<(), String> {
    let ctx = QuotientCtx::new(p);
    let atlas_sigs: Vec<_> = bank
        .iter()
        .map(|(state, _)| ctx.c2.project(state))
        .collect();
    let atlas_table = ctx.c2.distance_from_sources(&atlas_sigs);

    let residue_ids = slice_ids()?;
    let residue = summarize_population(&ctx, starts, bank, &atlas_table, &residue_ids)?;
    let solved_ids: BTreeSet<String> = submission_entries()?
        .into_iter()
        .map(|(id, _)| id)
        .collect();
    let solved_vec: Vec<String> = solved_ids.into_iter().collect();
    let solved = summarize_population(&ctx, starts, bank, &atlas_table, &solved_vec)?;

    println!(
        "starts p={p}: residue n={} c2_dist mean={:.2} p50={} le3={} le7={} | solved n={} c2_dist mean={:.2} le7={}",
        residue_ids.len(),
        hist_stats(&residue.c2_hist).4,
        hist_stats(&residue.c2_hist).2,
        count_at_most(&residue.c2_hist, 3),
        count_at_most(&residue.c2_hist, 7),
        solved_vec.len(),
        hist_stats(&solved.c2_hist).4,
        count_at_most(&solved.c2_hist, 7),
    );
    println!(
        "  residue atlas states: {}/{} (mean suffix {:.0}) | solved atlas states: {}/{} (mean suffix {:.0})",
        residue.atlas_states,
        residue_ids.len(),
        residue.atlas_suffix_total as f64 / residue.atlas_states.max(1) as f64,
        solved.atlas_states,
        solved_vec.len(),
        solved.atlas_suffix_total as f64 / solved.atlas_states.max(1) as f64,
    );
    let summary = json!({
        "format_version": 1,
        "phase": "starts",
        "p": p,
        "class2_space": ctx.c2.space(),
        "abelian_space": ctx.ab.space(),
        "atlas_signatures": atlas_sigs.len(),
        "residue": {
            "n": residue_ids.len(),
            "c2_hist": dist_rows(&residue.c2_hist),
            "c2_stats": stats_json(&residue.c2_hist),
            "ab_hist": dist_rows(&residue.ab_hist),
            "ab_stats": stats_json(&residue.ab_hist),
            "start_len_hist": dist_rows(&residue.len_hist),
            "atlas_states": residue.atlas_states,
            "dist_to_atlas_sig_hist": dist_rows(&residue.atlas_hist),
            "c2_le_3": count_at_most(&residue.c2_hist, 3),
            "c2_le_7": count_at_most(&residue.c2_hist, 7),
        },
        "solved": {
            "n": solved_vec.len(),
            "c2_hist": dist_rows(&solved.c2_hist),
            "c2_stats": stats_json(&solved.c2_hist),
            "ab_hist": dist_rows(&solved.ab_hist),
            "ab_stats": stats_json(&solved.ab_hist),
            "start_len_hist": dist_rows(&solved.len_hist),
            "atlas_states": solved.atlas_states,
            "dist_to_atlas_sig_hist": dist_rows(&solved.atlas_hist),
            "c2_le_3": count_at_most(&solved.c2_hist, 3),
            "c2_le_7": count_at_most(&solved.c2_hist, 7),
        },
        "rows": residue.rows,
        "solved_rows": solved.rows,
    });
    write_json(&format!("s4_quotient_p{p}_starts.json"), &summary)
}

/// Atlas projection: occupancy, suffix-length correlation, and the hard
/// consistency check `abstract distance <= known suffix length` on every
/// banked state.
fn phase_atlas(p: u8, bank: &GoalBank) -> Result<(), String> {
    let ctx = QuotientCtx::new(p);
    let mut occupied = vec![false; ctx.c2.space()];
    let mut qdist_hist: BTreeMap<u16, u64> = BTreeMap::new();
    let mut suffix_hist: BTreeMap<u16, u64> = BTreeMap::new();
    let mut qdists: Vec<u32> = Vec::with_capacity(bank.len());
    let mut suffix_lens: Vec<u32> = Vec::with_capacity(bank.len());
    let mut groups: HashMap<usize, (u64, u64, u64)> = HashMap::new(); // sum, sumsq, n
    let mut suffix_by_qdist: BTreeMap<u16, Vec<u16>> = BTreeMap::new();
    let mut violations = 0u64;
    for (state, suffix) in bank.iter() {
        let idx = ctx.c2.index(&ctx.c2.project(state));
        let dist = ctx.c2_target.get(idx);
        occupied[idx] = true;
        *qdist_hist.entry(dist).or_default() += 1;
        *suffix_hist.entry(suffix.len() as u16).or_default() += 1;
        if (dist as usize) > suffix.len() {
            violations += 1;
        }
        qdists.push(dist as u32);
        suffix_lens.push(suffix.len() as u32);
        let group = groups.entry(idx).or_insert((0, 0, 0));
        group.0 += suffix.len() as u64;
        group.1 += (suffix.len() as u64).pow(2);
        group.2 += 1;
        suffix_by_qdist
            .entry(dist)
            .or_default()
            .push(suffix.len() as u16);
    }
    let occupied_count = occupied.iter().filter(|x| **x).count();
    let overall_mean = suffix_lens.iter().map(|&x| x as f64).sum::<f64>() / bank.len() as f64;
    let ss_total: f64 = suffix_lens
        .iter()
        .map(|&x| (x as f64 - overall_mean).powi(2))
        .sum();
    let ss_within: f64 = groups
        .values()
        .map(|&(sum, sumsq, n)| sumsq as f64 - (sum as f64).powi(2) / n as f64)
        .sum();
    let eta2 = 1.0 - ss_within / ss_total;
    let rho = spearman(&qdists, &suffix_lens);
    let suffix_by_qdist_rows: Vec<Value> = suffix_by_qdist
        .iter_mut()
        .map(|(&dist, lens)| {
            lens.sort_unstable();
            let n = lens.len();
            let mean = lens.iter().map(|&x| x as f64).sum::<f64>() / n as f64;
            json!({
                "qdist": dist,
                "states": n,
                "mean_suffix": mean,
                "min_suffix": lens[0],
                "p50_suffix": lens[n / 2],
                "max_suffix": lens[n - 1],
            })
        })
        .collect();
    let (_, _, qmedian, _, qmean) = hist_stats(&qdist_hist);
    println!(
        "atlas p={p}: states={} occupied={}/{} ({:.1}%) c2_dist mean={:.2} p50={} violations(qdist>suffix)={} spearman={:.3} eta2={:.3}",
        bank.len(),
        occupied_count,
        ctx.c2.space(),
        100.0 * occupied_count as f64 / ctx.c2.space() as f64,
        qmean,
        qmedian,
        violations,
        rho,
        eta2,
    );
    let summary = json!({
        "format_version": 1,
        "phase": "atlas",
        "p": p,
        "class2_space": ctx.c2.space(),
        "states": bank.len(),
        "occupied": occupied_count,
        "occupancy": occupied_count as f64 / ctx.c2.space() as f64,
        "qdist_hist": dist_rows(&qdist_hist),
        "qdist_stats": stats_json(&qdist_hist),
        "suffix_len_hist": dist_rows(&suffix_hist),
        "suffix_len_stats": stats_json(&suffix_hist),
        "violations_qdist_gt_suffix": violations,
        "spearman_qdist_suffix": rho,
        "eta2_signature_explains_suffix": eta2,
        "suffix_by_qdist": suffix_by_qdist_rows,
    });
    write_json(&format!("s4_quotient_p{p}_atlas.json"), &summary)
}

/// Verified-trajectory projection: does abstract distance fall along real
/// solution paths, and how often does it plateau?
fn phase_traj(p: u8, starts: &HashMap<String, Presentation>) -> Result<(), String> {
    let ctx = QuotientCtx::new(p);
    let mut hist: BTreeMap<(u16, u16), u64> = BTreeMap::new();
    let mut delta_hist: BTreeMap<i32, u64> = BTreeMap::new();
    let (mut dec, mut eq, mut inc) = (0u64, 0u64, 0u64);
    let (mut c2_violations, mut ab_violations) = (0u64, 0u64);
    let mut certs = 0u64;
    let mut states_total = 0u64;
    for (id, cert) in submission_entries()? {
        let start = starts
            .get(&id)
            .ok_or_else(|| format!("unknown challenge {id}"))?;
        let states = replay_states(start, &cert)?;
        certs += 1;
        let mut prev: Option<u16> = None;
        for (i, state) in states.iter().enumerate() {
            let remaining = (states.len() - 1 - i) as u16;
            let dist = ctx.c2_dist(state);
            if dist > remaining {
                c2_violations += 1;
            }
            let ad = ctx.ab_dist(state);
            if ad > dist {
                ab_violations += 1;
            }
            *hist
                .entry((state.total_relator_length() as u16, dist))
                .or_default() += 1;
            if let Some(prev) = prev {
                match dist.cmp(&prev) {
                    std::cmp::Ordering::Less => {
                        dec += 1;
                        *delta_hist.entry(dist as i32 - prev as i32).or_default() += 1;
                    }
                    std::cmp::Ordering::Equal => eq += 1,
                    std::cmp::Ordering::Greater => {
                        inc += 1;
                        *delta_hist.entry(dist as i32 - prev as i32).or_default() += 1;
                    }
                }
            }
            prev = Some(dist);
            states_total += 1;
        }
    }
    let transitions = dec + eq + inc;
    println!(
        "traj p={p}: certs={certs} states={states_total} transitions={transitions} dec={dec} ({:.1}%) eq={eq} inc={inc} c2>remaining={c2_violations} ab>c2={ab_violations}",
        if transitions == 0 {
            f64::NAN
        } else {
            100.0 * dec as f64 / transitions as f64
        },
    );
    let delta_rows: Vec<Value> = delta_hist
        .iter()
        .map(|(&delta, &count)| json!({"delta": delta, "count": count}))
        .collect();
    let summary = json!({
        "format_version": 1,
        "phase": "traj",
        "p": p,
        "certs": certs,
        "states": states_total,
        "transitions": transitions,
        "decreased": dec,
        "equal": eq,
        "increased": inc,
        "delta_hist": delta_rows,
        "violations_c2_gt_remaining": c2_violations,
        "violations_ab_gt_c2": ab_violations,
        "hist_len_qdist": len_qdist_rows(&hist),
        "buckets": bucket_summary(&hist, 5),
    });
    write_json(&format!("s4_quotient_p{p}_traj.json"), &summary)
}

/// Failed-search sampling: run the frozen medium-budget bank-greedy over the
/// residue slice with a discovery observer, and record sampled states by
/// (total relator length, abstract distance). Solved runs go to their own
/// histogram; the comparison target is verified-trajectory states.
fn phase_failed(
    p: u8,
    starts: &HashMap<String, Presentation>,
    bank: &GoalBank,
) -> Result<(), String> {
    let ctx = QuotientCtx::new(p);
    let limits = SearchLimits {
        max_expanded: None,
        max_depth: None,
        max_total_relator_length: Some(10_000),
        max_work: Some(MEDIUM_WORK),
    };
    let mut hist_failed: BTreeMap<(u16, u16), u64> = BTreeMap::new();
    let mut hist_solved: BTreeMap<(u16, u16), u64> = BTreeMap::new();
    let mut case_rows = Vec::new();
    let (mut solved_cases, mut sampled) = (0u64, 0u64);
    for id in slice_ids()? {
        let start = starts
            .get(&id)
            .ok_or_else(|| format!("unknown challenge {id}"))?;
        let start_dist = ctx.c2_dist(start);
        let mut case_hist: BTreeMap<(u16, u16), u64> = BTreeMap::new();
        let outcome = {
            let mut counter = 0u64;
            let mut on_discover = |state: &Presentation, _depth: u32| {
                counter += 1;
                if counter.is_multiple_of(SAMPLE_STRIDE) {
                    let dist = ctx.c2_dist(state);
                    *case_hist
                        .entry((state.total_relator_length() as u16, dist))
                        .or_default() += 1;
                }
            };
            solve_bank_greedy_observed(start, &limits, bank, None, Some(&mut on_discover))
        };
        let solved = matches!(outcome.result.status, SearchStatus::Solved);
        if solved {
            solved_cases += 1;
        }
        sampled += case_hist.values().sum::<u64>();
        let target = if solved {
            &mut hist_solved
        } else {
            &mut hist_failed
        };
        for (key, count) in case_hist {
            *target.entry(key).or_default() += count;
        }
        let status = match &outcome.result.status {
            SearchStatus::Solved => "solved",
            SearchStatus::Exhausted => "exhausted",
            SearchStatus::LimitReached => "limit",
            SearchStatus::Unimplemented => "unimplemented",
            SearchStatus::Failed(msg) => return Err(format!("{id} failed: {msg}")),
        };
        case_rows.push(json!({
            "id": id,
            "start_len": start.total_relator_length(),
            "start_c2_dist": start_dist,
            "status": status,
            "solved": solved,
            "via_bank": outcome.via_bank,
            "via_ball": outcome.via_ball,
            "ball_hit_depth": outcome.ball_hit_depth,
            "generated": outcome.result.metrics.generated,
            "unique": outcome.result.metrics.unique,
        }));
    }
    println!(
        "failed p={p}: cases={} solved={} sampled={}",
        case_rows.len(),
        solved_cases,
        sampled,
    );
    let summary = json!({
        "format_version": 1,
        "phase": "failed",
        "p": p,
        "budget_generated": MEDIUM_WORK,
        "sample_stride": SAMPLE_STRIDE,
        "cases": case_rows.len(),
        "solved": solved_cases,
        "hist_failed": len_qdist_rows(&hist_failed),
        "hist_solved_runs": len_qdist_rows(&hist_solved),
        "buckets_failed": bucket_summary(&hist_failed, 5),
        "buckets_solved_runs": bucket_summary(&hist_solved, 5),
        "case_rows": case_rows,
    });
    write_json(&format!("s4_quotient_p{p}_failed.json"), &summary)
}

fn main() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    let phase = args.next().unwrap_or_else(|| "all".to_string());
    let p: u8 = args
        .next()
        .map(|s| s.parse().expect("p must be an integer"))
        .unwrap_or(5);
    let starts = load_starts()?;
    let bank = build_bank(&starts)?;
    eprintln!("starts: {}  bank: {} states", starts.len(), bank.len());
    match phase.as_str() {
        "starts" => phase_starts(p, &starts, &bank)?,
        "atlas" => phase_atlas(p, &bank)?,
        "traj" => phase_traj(p, &starts)?,
        "failed" => phase_failed(p, &starts, &bank)?,
        "all" => {
            phase_atlas(p, &bank)?;
            phase_starts(p, &starts, &bank)?;
            phase_traj(p, &starts)?;
            phase_failed(p, &starts, &bank)?;
        }
        other => return Err(format!("unknown phase {other}")),
    }
    Ok(())
}
