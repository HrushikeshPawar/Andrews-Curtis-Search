//! S4.3 experiment: witnessed ACSolverX-style substitution macros.
//!
//! A substitution writes a cancellation-producing product of cyclic
//! rotations into one relator and leaves the other relator unchanged.
//! Rotations, optional inversion, left/right multiply, source restore, and
//! wrap-around cyclic reduction are all official primitive witnesses.
//! Booth canonicalization is not applied. See
//! `docs/analysis/s4-3-substitution-witness.md`.
//!
//! Frozen primitive greedy/beam/S4.2b supermove solvers are untouched.

use super::atlas_search::AtlasOutcome;
use super::goalbank::GoalBank;
use super::greedy::HeapItem;
use super::store::{StateId, StateStore};
use super::supermove::inverse_witness;
use super::{SearchLimits, SearchMetrics};
use super::{exhausted, failed, limit_reached, reduce_presentation, solved};
use ac_core::{
    DEFAULT_N_GEN, Move, MoveId, Presentation, apply_move, concat, inverse, is_ac_target,
    move_from_id, replay_certificate,
};
use std::collections::{BinaryHeap, HashMap};
use std::time::Instant;

/// How many cancelling substitutions to keep per expansion.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SubstitutionPolicy {
    /// Cancelling unique successors kept per expansion (`0` = keep all).
    pub top_k: usize,
}

impl Default for SubstitutionPolicy {
    fn default() -> Self {
        Self { top_k: 16 }
    }
}

/// One witnessed substitution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubstitutionProposal {
    pub successor: Presentation,
    pub witness: Vec<MoveId>,
    /// Signed net change in total relator length (positive = shorter).
    pub cancelled: i64,
    /// Letters removed while forming the written product.
    pub letters_removed: u32,
    pub target: usize,
    pub left_multiply: bool,
    pub invert_other: bool,
    pub rot_target: usize,
    pub rot_other: usize,
}

fn conjugation_move_id(target: usize, letter: i8) -> MoveId {
    let base: MoveId = match target {
        0 => 6,
        1 => 10,
        _ => panic!("relator index out of range"),
    };
    base + match letter {
        1 => 0,
        -1 => 1,
        2 => 2,
        -2 => 3,
        _ => panic!("conjugator must be ±1 or ±2"),
    }
}

fn invert_move_id(target: usize) -> MoveId {
    match target {
        0 => 0,
        1 => 1,
        _ => panic!("relator index out of range"),
    }
}

fn multiply_move_id(target: usize, invert_other: bool) -> MoveId {
    match (target, invert_other) {
        (0, false) => 2,
        (0, true) => 3,
        (1, false) => 4,
        (1, true) => 5,
        _ => panic!("relator index out of range"),
    }
}

fn left_rot(word: &[i8], k: usize) -> Vec<i8> {
    if word.is_empty() {
        return Vec::new();
    }
    let k = k % word.len();
    let mut out = Vec::with_capacity(word.len());
    out.extend_from_slice(&word[k..]);
    out.extend_from_slice(&word[..k]);
    out
}

fn left_rotate_witness(target: usize, word: &[i8], k: usize) -> Vec<MoveId> {
    if word.is_empty() {
        return Vec::new();
    }
    let k = k % word.len();
    word[..k]
        .iter()
        .map(|&letter| conjugation_move_id(target, -letter))
        .collect()
}

fn cyclic_reduce_word(word: &[i8]) -> Vec<i8> {
    let mut w = word.to_vec();
    while w.len() >= 2 && w[0] == -w[w.len() - 1] {
        w.pop();
        w.remove(0);
    }
    w
}

fn cyclic_reduce_witness(target: usize, word: &[i8]) -> Vec<MoveId> {
    let mut w = word.to_vec();
    let mut out = Vec::new();
    while w.len() >= 2 && w[0] == -w[w.len() - 1] {
        let last = *w.last().expect("len >= 2");
        out.push(conjugation_move_id(target, last));
        w.pop();
        w.remove(0);
    }
    out
}

fn apply_witness(start: &Presentation, witness: &[MoveId]) -> Option<Presentation> {
    let table: [Move; 14] =
        core::array::from_fn(|i| move_from_id(i as u8).expect("ids 0..=13 are pinned"));
    let mut state = start.clone();
    for &id in witness {
        match apply_move(&state, &table[id as usize], DEFAULT_N_GEN) {
            Ok(next) => state = next,
            Err(_) => return None,
        }
    }
    Some(state)
}

struct Align {
    target: usize,
    invert_other: bool,
    kt: usize,
    ko: usize,
    left_multiply: bool,
}

/// Primitive witness for one substitution, not including cyclic reduction.
fn multiply_witness(t_word: &[i8], o_word: &[i8], align: Align) -> Vec<MoveId> {
    let Align {
        target,
        invert_other,
        kt,
        ko,
        left_multiply,
    } = align;
    let other = 1 - target;
    let mut witness = Vec::new();
    witness.extend(left_rotate_witness(target, t_word, kt));
    let o_use = if invert_other {
        inverse(o_word, DEFAULT_N_GEN).expect("stored relators are valid")
    } else {
        o_word.to_vec()
    };
    if !left_multiply && ko == 0 {
        witness.push(multiply_move_id(target, invert_other));
        return witness;
    }
    if invert_other {
        witness.push(invert_move_id(other));
    }
    let rot_other = left_rotate_witness(other, &o_use, ko);
    witness.extend_from_slice(&rot_other);
    if left_multiply {
        witness.push(invert_move_id(target));
        witness.push(multiply_move_id(target, true));
        witness.push(invert_move_id(target));
    } else {
        witness.push(multiply_move_id(target, false));
    }
    witness.extend(inverse_witness(&rot_other));
    if invert_other {
        witness.push(invert_move_id(other));
    }
    witness
}

/// Enumerate cancelling substitutions, deduplicated by successor
/// (best net cancellation, then letters removed, then shortest witness)
/// and ordered deterministically.
pub fn propose_all(pres: &Presentation, _policy: &SubstitutionPolicy) -> Vec<SubstitutionProposal> {
    let base_len = pres.total_relator_length() as i64;
    let mut best: HashMap<Presentation, SubstitutionProposal> = HashMap::new();
    for target in 0..2usize {
        let other = 1 - target;
        let t_word = &pres.relators[target];
        let o_word = &pres.relators[other];
        let n_t = t_word.len().max(1);
        for invert_other in [false, true] {
            let o_use = if invert_other {
                inverse(o_word, DEFAULT_N_GEN).expect("stored relators are valid")
            } else {
                o_word.clone()
            };
            let n_o = o_use.len().max(1);
            for kt in 0..n_t {
                let t_rot = left_rot(t_word, kt);
                for ko in 0..n_o {
                    let o_rot = left_rot(&o_use, ko);
                    for left_multiply in [false, true] {
                        let joined = if left_multiply {
                            concat(&o_rot, &t_rot, DEFAULT_N_GEN)
                        } else {
                            concat(&t_rot, &o_rot, DEFAULT_N_GEN)
                        }
                        .expect("rotations of valid relators concat");
                        let product = cyclic_reduce_word(&joined);
                        let letters_removed =
                            (t_rot.len() + o_rot.len()).saturating_sub(product.len()) as u32;
                        if letters_removed == 0 || product == pres.relators[target] {
                            continue;
                        }
                        let mut witness = multiply_witness(
                            t_word,
                            o_word,
                            Align {
                                target,
                                invert_other,
                                kt,
                                ko,
                                left_multiply,
                            },
                        );
                        witness.extend(cyclic_reduce_witness(target, &joined));
                        let successor = match apply_witness(pres, &witness) {
                            Some(state) if state != *pres => state,
                            _ => continue,
                        };
                        if successor.relators[target] != product
                            || successor.relators[other] != pres.relators[other]
                        {
                            continue;
                        }
                        let entry = SubstitutionProposal {
                            cancelled: base_len - successor.total_relator_length() as i64,
                            letters_removed,
                            successor,
                            witness,
                            target,
                            left_multiply,
                            invert_other,
                            rot_target: kt,
                            rot_other: ko,
                        };
                        let replace = match best.get(&entry.successor) {
                            None => true,
                            Some(old) => {
                                entry.cancelled > old.cancelled
                                    || (entry.cancelled == old.cancelled
                                        && entry.letters_removed > old.letters_removed)
                                    || (entry.cancelled == old.cancelled
                                        && entry.letters_removed == old.letters_removed
                                        && entry.witness.len() < old.witness.len())
                            }
                        };
                        if replace {
                            best.insert(entry.successor.clone(), entry);
                        }
                    }
                }
            }
        }
    }
    let mut all: Vec<SubstitutionProposal> = best.into_values().collect();
    all.sort_by(|a, b| {
        b.cancelled
            .cmp(&a.cancelled)
            .then_with(|| b.letters_removed.cmp(&a.letters_removed))
            .then_with(|| a.witness.len().cmp(&b.witness.len()))
            .then_with(|| a.successor.relators.cmp(&b.successor.relators))
    });
    all
}

/// Keep the best `top_k` cancelling unique successors (`0` keeps all).
pub fn propose(pres: &Presentation, policy: &SubstitutionPolicy) -> Vec<SubstitutionProposal> {
    let all = propose_all(pres, policy);
    if policy.top_k == 0 {
        all
    } else {
        all.into_iter().take(policy.top_k).collect()
    }
}

fn reconstruct_macros(
    parents: &[Option<(StateId, u32)>],
    witnesses: &[Vec<MoveId>],
    end: StateId,
) -> Vec<MoveId> {
    let mut chain = Vec::new();
    let mut cur = end;
    while let Some((parent, witness)) = parents[cur as usize] {
        chain.push(witness);
        cur = parent;
    }
    chain.reverse();
    let mut out = Vec::new();
    for witness in chain {
        out.extend_from_slice(&witnesses[witness as usize]);
    }
    out
}

/// Best-first search over witnessed substitution edges only. Priority is
/// total relator length, then discovery sequence. Bank probing matches
/// `atlas_search`; reverse-ball is not used. Each macro is charged
/// `witness.len()` against the work budget.
pub(crate) fn solve_substitution_greedy(
    start: &Presentation,
    limits: &SearchLimits,
    policy: &SubstitutionPolicy,
    bank: &GoalBank,
) -> AtlasOutcome {
    let done = |result, via_bank: bool| AtlasOutcome {
        result,
        via_bank,
        via_ball: false,
        ball_hit_depth: None,
    };
    let t0 = Instant::now();
    let root = match reduce_presentation(start) {
        Ok(root) => root,
        Err(msg) => {
            return done(failed(msg, SearchMetrics::default(), t0.elapsed()), false);
        }
    };
    let mut metrics = SearchMetrics {
        max_total_relator_length: root.total_relator_length(),
        ..SearchMetrics::default()
    };
    if is_ac_target(&root) {
        metrics.unique = 1;
        return done(solved(&root, Vec::new(), metrics, t0.elapsed()), false);
    }
    if let Some(suffix) = bank.suffix(&root) {
        metrics.unique = 1;
        return done(solved(&root, suffix.to_vec(), metrics, t0.elapsed()), true);
    }
    let mut store = StateStore::new();
    store
        .insert_new(&root.relators[0], &root.relators[1], None, 0)
        .expect("fresh store accepts the root");
    let mut parents: Vec<Option<(StateId, u32)>> = vec![None];
    let mut witnesses: Vec<Vec<MoveId>> = Vec::new();
    let mut heap: BinaryHeap<HeapItem> = BinaryHeap::new();
    heap.push(HeapItem {
        length: root.total_relator_length(),
        seq: 0,
        id: 0,
    });
    let mut seq: u64 = 1;
    metrics.unique = 1;
    metrics.max_frontier = 1;
    let (mut w0, mut w1) = (Vec::new(), Vec::new());
    let mut pruned = false;
    while let Some(item) = heap.pop() {
        if limits
            .max_expanded
            .is_some_and(|cap| metrics.expanded >= cap)
        {
            return done(limit_reached(metrics, t0.elapsed()), false);
        }
        let depth = store.depth(item.id);
        let producer_inverse: Option<Vec<MoveId>> = parents[item.id as usize]
            .map(|(_, witness)| inverse_witness(&witnesses[witness as usize]));
        store.copy_state(item.id, &mut w0, &mut w1);
        let current = Presentation {
            relators: [std::mem::take(&mut w0), std::mem::take(&mut w1)],
        };
        for proposal in propose(&current, policy) {
            if producer_inverse
                .as_deref()
                .is_some_and(|inv| inv == proposal.witness)
            {
                continue;
            }
            let cost = proposal.witness.len() as u64;
            if limits
                .max_work
                .is_some_and(|cap| metrics.generated + cost > cap)
            {
                return done(limit_reached(metrics, t0.elapsed()), false);
            }
            metrics.generated += cost;
            metrics.macros += 1;
            metrics.macro_witness_moves += cost;
            metrics.macro_cancelled += proposal.cancelled.max(0) as u64;
            let SubstitutionProposal {
                successor, witness, ..
            } = proposal;
            let length = successor.total_relator_length();
            metrics.max_total_relator_length = metrics.max_total_relator_length.max(length);
            if store.contains(&successor.relators[0], &successor.relators[1]) {
                metrics.duplicates += 1;
                continue;
            }
            let child_depth = depth + 1;
            if limits
                .max_total_relator_length
                .is_some_and(|cap| length > cap)
                || limits.max_depth.is_some_and(|cap| child_depth > cap)
            {
                pruned = true;
                continue;
            }
            let child = store
                .insert_new(
                    &successor.relators[0],
                    &successor.relators[1],
                    None,
                    child_depth,
                )
                .expect("probed-absent state must insert");
            parents.push(Some((item.id, witnesses.len() as u32)));
            witnesses.push(witness);
            metrics.unique += 1;
            metrics.max_depth_seen = metrics.max_depth_seen.max(child_depth);
            if is_ac_target(&successor) {
                let certificate = reconstruct_macros(&parents, &witnesses, child);
                return done(solved(&root, certificate, metrics, t0.elapsed()), false);
            }
            if let Some(suffix) = bank.suffix(&successor) {
                let mut certificate = reconstruct_macros(&parents, &witnesses, child);
                certificate.extend_from_slice(suffix);
                return done(solved(&root, certificate, metrics, t0.elapsed()), true);
            }
            heap.push(HeapItem {
                length,
                seq,
                id: child,
            });
            seq += 1;
            metrics.max_frontier = metrics.max_frontier.max(heap.len() as u64);
        }
        metrics.expanded += 1;
        let Presentation { relators: [b0, b1] } = current;
        w0 = b0;
        w1 = b1;
    }
    if pruned {
        done(limit_reached(metrics, t0.elapsed()), false)
    } else {
        done(exhausted(metrics, t0.elapsed()), false)
    }
}

/// Quality-mode substitution search. Unlike the frozen coverage solver above,
/// this continues after bank hits and keeps the shortest replay-verified
/// candidate below `incumbent_len`.
pub fn solve_substitution_quality(
    start: &Presentation,
    limits: &SearchLimits,
    policy: &SubstitutionPolicy,
    bank: &GoalBank,
    incumbent_len: usize,
) -> AtlasOutcome {
    let done = |result, via_bank: bool| AtlasOutcome {
        result,
        via_bank,
        via_ball: false,
        ball_hit_depth: None,
    };
    let t0 = Instant::now();
    let root = match reduce_presentation(start) {
        Ok(root) => root,
        Err(msg) => return done(failed(msg, SearchMetrics::default(), t0.elapsed()), false),
    };
    let mut metrics = SearchMetrics {
        max_total_relator_length: root.total_relator_length(),
        ..SearchMetrics::default()
    };
    let mut best: Option<Vec<MoveId>> = None;
    let mut best_via_bank = false;
    let mut consider = |candidate: Vec<MoveId>, via_bank: bool| {
        let bound = best.as_ref().map_or(incumbent_len, Vec::len);
        if candidate.len() < bound && replay_certificate(&root, &candidate).is_ok() {
            best = Some(candidate);
            best_via_bank = via_bank;
        }
    };
    if is_ac_target(&root) {
        consider(Vec::new(), false);
    }
    if let Some(suffix) = bank.suffix(&root) {
        consider(suffix.to_vec(), true);
    }

    let mut store = StateStore::new();
    store
        .insert_new(&root.relators[0], &root.relators[1], None, 0)
        .expect("fresh store accepts the root");
    let mut parents: Vec<Option<(StateId, u32)>> = vec![None];
    let mut witnesses: Vec<Vec<MoveId>> = Vec::new();
    let mut heap: BinaryHeap<HeapItem> = BinaryHeap::new();
    heap.push(HeapItem {
        length: root.total_relator_length(),
        seq: 0,
        id: 0,
    });
    let mut seq = 1;
    let mut pruned = false;
    let (mut w0, mut w1) = (Vec::new(), Vec::new());
    metrics.unique = 1;
    metrics.max_frontier = 1;
    while let Some(item) = heap.pop() {
        if limits
            .max_expanded
            .is_some_and(|cap| metrics.expanded >= cap)
        {
            break;
        }
        let depth = store.depth(item.id);
        let producer_inverse = parents[item.id as usize]
            .map(|(_, witness)| inverse_witness(&witnesses[witness as usize]));
        store.copy_state(item.id, &mut w0, &mut w1);
        let current = Presentation {
            relators: [std::mem::take(&mut w0), std::mem::take(&mut w1)],
        };
        for proposal in propose(&current, policy) {
            if producer_inverse
                .as_deref()
                .is_some_and(|inverse| inverse == proposal.witness)
            {
                continue;
            }
            let cost = proposal.witness.len() as u64;
            if limits
                .max_work
                .is_some_and(|cap| metrics.generated + cost > cap)
            {
                pruned = true;
                break;
            }
            metrics.generated += cost;
            metrics.macros += 1;
            metrics.macro_witness_moves += cost;
            metrics.macro_cancelled += proposal.cancelled.max(0) as u64;
            let SubstitutionProposal {
                successor, witness, ..
            } = proposal;
            let length = successor.total_relator_length();
            metrics.max_total_relator_length = metrics.max_total_relator_length.max(length);
            if store.contains(&successor.relators[0], &successor.relators[1]) {
                metrics.duplicates += 1;
                continue;
            }
            let child_depth = depth + 1;
            if limits
                .max_total_relator_length
                .is_some_and(|cap| length > cap)
                || limits.max_depth.is_some_and(|cap| child_depth > cap)
            {
                pruned = true;
                continue;
            }
            let child = store
                .insert_new(
                    &successor.relators[0],
                    &successor.relators[1],
                    None,
                    child_depth,
                )
                .expect("probed-absent state must insert");
            parents.push(Some((item.id, witnesses.len() as u32)));
            witnesses.push(witness);
            metrics.unique += 1;
            metrics.max_depth_seen = metrics.max_depth_seen.max(child_depth);
            let prefix = reconstruct_macros(&parents, &witnesses, child);
            if is_ac_target(&successor) {
                consider(prefix.clone(), false);
            }
            if let Some(suffix) = bank.suffix(&successor) {
                let mut candidate = prefix;
                candidate.extend_from_slice(suffix);
                consider(candidate, true);
            }
            heap.push(HeapItem {
                length,
                seq,
                id: child,
            });
            seq += 1;
            metrics.max_frontier = metrics.max_frontier.max(heap.len() as u64);
        }
        metrics.expanded += 1;
        let Presentation { relators: [b0, b1] } = current;
        w0 = b0;
        w1 = b1;
    }
    match best {
        Some(candidate) => done(
            solved(&root, candidate, metrics, t0.elapsed()),
            best_via_bank,
        ),
        None if pruned || !heap.is_empty() => done(limit_reached(metrics, t0.elapsed()), false),
        None => done(exhausted(metrics, t0.elapsed()), false),
    }
}

#[cfg(test)]
mod substitution_tests {
    use super::*;
    use ac_core::replay_certificate;

    fn pres(r0: &[i8], r1: &[i8]) -> Presentation {
        Presentation {
            relators: [r0.to_vec(), r1.to_vec()],
        }
    }

    fn apply(start: &Presentation, witness: &[MoveId]) -> Presentation {
        apply_witness(start, witness).expect("official moves apply to reduced states")
    }

    #[test]
    fn quality_mode_improves_an_incumbent_after_a_bank_hit() {
        let start = pres(&[1, 2], &[2]);
        let mut bank = GoalBank::new();
        bank.insert_certificate(&start, &[3]).unwrap();
        let result = solve_substitution_quality(
            &start,
            &SearchLimits {
                max_expanded: Some(10),
                max_depth: Some(2),
                max_total_relator_length: Some(100),
                max_work: Some(1_000),
            },
            &SubstitutionPolicy::default(),
            &bank,
            3,
        );
        assert_eq!(result.result.certificate, vec![3]);
        assert!(result.result.metrics.replay_ok);
        assert_eq!(result.result.certificate_length, Some(1));
    }

    #[test]
    fn every_proposal_is_witnessed() {
        let states = [
            pres(&[1, 2], &[2]),
            pres(&[1], &[2, 1]),
            pres(&[2, 2, 1], &[-2, -1]),
            pres(&[2, 1, 2], &[-2]),
            pres(&[-1, -2], &[-1, -2, 1]),
            pres(&[1, -2, 1, 2, -1], &[2, 1, -2]),
        ];
        for state in states {
            for policy in [
                SubstitutionPolicy::default(),
                SubstitutionPolicy { top_k: 0 },
                SubstitutionPolicy { top_k: 4 },
            ] {
                for proposal in propose(&state, &policy) {
                    assert!(!proposal.witness.is_empty());
                    assert_eq!(
                        apply(&state, &proposal.witness),
                        proposal.successor,
                        "witness disagrees with successor"
                    );
                    assert_eq!(
                        proposal.cancelled,
                        state.total_relator_length() as i64
                            - proposal.successor.total_relator_length() as i64
                    );
                    let other = 1 - proposal.target;
                    assert_eq!(
                        proposal.successor.relators[other], state.relators[other],
                        "other relator must be restored"
                    );
                    assert!(proposal.letters_removed > 0);
                }
            }
        }
    }

    #[test]
    fn direct_right_multiply_reaches_target() {
        let proposals = propose(&pres(&[1, 2], &[2]), &SubstitutionPolicy { top_k: 0 });
        assert!(
            proposals.iter().any(|m| m.witness == vec![3]
                && m.cancelled == 1
                && m.successor == pres(&[1], &[2])),
            "missing move-3 proposal: {proposals:?}"
        );
        let proposals = propose(&pres(&[1], &[2, 1]), &SubstitutionPolicy { top_k: 0 });
        assert!(
            proposals
                .iter()
                .any(|m| m.witness == vec![5] && m.successor == pres(&[1], &[2])),
            "missing move-5 proposal: {proposals:?}"
        );
    }

    #[test]
    fn wrap_around_cyclic_reduce_restores_other() {
        // rot(t,1)=[2,1,2], rot(s,1)=[-1,-2], concat has no junction cancel
        // but wrap-around cyclic-reduces to [2]; s is restored.
        let state = pres(&[2, 2, 1], &[-2, -1]);
        let proposals = propose(&state, &SubstitutionPolicy { top_k: 0 });
        assert!(
            proposals
                .iter()
                .any(|m| m.successor == pres(&[2], &[-2, -1])
                    && m.target == 0
                    && !m.left_multiply
                    && m.letters_removed >= 4),
            "missing wrap-around proposal: {proposals:?}"
        );
    }

    #[test]
    fn booth_lex_min_is_not_applied() {
        // Identity-alignment multiply of [[2,1,2],[-2]] writes [2,1], not the
        // Booth representative [1,2] of that same product. (A different
        // rotation may legitimately write [1,2]; that is not Booth.)
        let state = pres(&[2, 1, 2], &[-2]);
        let proposals = propose(&state, &SubstitutionPolicy { top_k: 0 });
        let identity = proposals.iter().find(|m| {
            m.target == 0
                && !m.left_multiply
                && !m.invert_other
                && m.rot_target == 0
                && m.rot_other == 0
        });
        assert!(
            identity.is_some(),
            "missing identity-alignment proposal: {proposals:?}"
        );
        assert_eq!(identity.unwrap().successor, pres(&[2, 1], &[-2]));
    }

    #[test]
    fn left_multiply_gadget_replays() {
        // Direct gadget: r0 <- r1 * r0 is invert, multiply-invert-other, invert.
        let state = pres(&[1, 2], &[2]);
        let witness = multiply_witness(
            &[1, 2],
            &[2],
            Align {
                target: 0,
                invert_other: false,
                kt: 0,
                ko: 0,
                left_multiply: true,
            },
        );
        assert_eq!(witness, vec![0, 3, 0]);
        assert_eq!(apply(&state, &witness), pres(&[2, 1, 2], &[2]));
    }

    #[test]
    fn two_step_macros_solve_and_replay() {
        let start = pres(&[1, 2, 2], &[2]);
        let outcome = solve_substitution_greedy(
            &start,
            &SearchLimits::default(),
            &SubstitutionPolicy { top_k: 0 },
            &GoalBank::new(),
        );
        assert!(matches!(outcome.result.status, crate::SearchStatus::Solved));
        assert!(outcome.result.metrics.replay_ok);
        assert!(!outcome.via_bank);
        let end = replay_certificate(&start, &outcome.result.certificate).unwrap();
        assert!(is_ac_target(&end));
        assert!(outcome.result.metrics.macros > 0);
        assert!(outcome.result.metrics.macro_witness_moves <= outcome.result.metrics.generated);
        assert_eq!(
            outcome.result.metrics.macro_witness_moves, outcome.result.metrics.generated,
            "macros-only search charges only witness moves"
        );
    }

    #[test]
    fn solver_is_deterministic() {
        let start = pres(&[1, 2, 2], &[2]);
        let a = solve_substitution_greedy(
            &start,
            &SearchLimits::default(),
            &SubstitutionPolicy::default(),
            &GoalBank::new(),
        );
        let b = solve_substitution_greedy(
            &start,
            &SearchLimits::default(),
            &SubstitutionPolicy::default(),
            &GoalBank::new(),
        );
        assert_eq!(a.result.certificate, b.result.certificate);
        assert_eq!(a.result.metrics.generated, b.result.metrics.generated);
        assert_eq!(a.result.metrics.unique, b.result.metrics.unique);
        assert_eq!(a.result.metrics.macros, b.result.metrics.macros);
    }

    #[test]
    fn length_two_is_not_an_ac_target() {
        // ACSolverX would accept [[2],[-2]] as trivial (two letters). We must not.
        let start = pres(&[2], &[-2]);
        let outcome = solve_substitution_greedy(
            &start,
            &SearchLimits {
                max_expanded: Some(20),
                max_depth: Some(4),
                max_total_relator_length: Some(20),
                max_work: Some(200),
            },
            &SubstitutionPolicy { top_k: 0 },
            &GoalBank::new(),
        );
        assert!(!matches!(
            outcome.result.status,
            crate::SearchStatus::Solved
        ));
    }
}
