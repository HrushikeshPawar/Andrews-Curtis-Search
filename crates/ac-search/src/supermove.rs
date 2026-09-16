//! S4.2b experiment: witnessed substitution supermoves.
//!
//! A supermove is a short primitive witness whose net effect removes letters
//! from the presentation. The canonical shape is a cyclic-ring substitution:
//! conjugate the target relator (and/or the source relator) by a short suffix
//! or inverse prefix, which cyclically rotates it so a different subword sits
//! at the junction, then multiply the rotated source in. If the junction words
//! match, the shared letters cancel and the witness is a cancellation-seeking
//! composite step that a length-greedy walk cannot take without visiting
//! longer intermediate states. The rotation is consumed by the cancellation,
//! so there is no un-rotation.
//!
//! Every supermove stores its exact successor and its full primitive witness,
//! and the search charges `witness.len()` against the work budget: one macro is
//! never one generated edge.
//!
//! Proposals are ranked by letters cancelled per witness move, plus a small
//! quota of length-non-decreasing proposals so the search can set up a
//! cancellation it cannot take yet.
//!
//! Like `atlas_search`, this module is an experiment vehicle: frozen solvers
//! are untouched and every certificate still goes through the normal replay
//! gate.

use super::atlas_search::{AtlasOutcome, inverse_table};
use super::ball::ReverseBall;
use super::goalbank::GoalBank;
use super::greedy::HeapItem;
use super::store::{StateId, StateStore};
use super::{SearchLimits, SearchMetrics};
use super::{exhausted, failed, limit_reached, reduce_presentation, solved};
use ac_core::{
    DEFAULT_N_GEN, Move, MoveId, Presentation, all_move_ids, apply_move, is_ac_target, move_from_id,
};
use std::collections::{BinaryHeap, HashMap};
use std::time::Instant;

/// How many and which supermoves to propose per expansion.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SupermovePolicy {
    /// Maximum rotation length per side (0 = direct appends only).
    pub max_conjugator: u8,
    /// Cancellation-producing proposals kept per expansion.
    pub top_k: usize,
    /// Additional length-non-decreasing proposals kept per expansion.
    pub keep_increasing: usize,
}

impl Default for SupermovePolicy {
    fn default() -> Self {
        Self {
            max_conjugator: 1,
            top_k: 8,
            keep_increasing: 2,
        }
    }
}

/// One witnessed composite step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Supermove {
    pub successor: Presentation,
    pub witness: Vec<MoveId>,
    /// Signed net change in total relator length (positive = cancelled).
    pub cancelled: i64,
}

/// Official conjugation move id for `target` (0 or 1) by one letter.
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

/// Official multiply move id appending `other = 1 - target` to `target`,
/// optionally inverted.
fn multiply_move_id(target: usize, invert_other: bool) -> MoveId {
    match (target, invert_other) {
        (0, false) => 2,
        (0, true) => 3,
        (1, false) => 4,
        (1, true) => 5,
        _ => panic!("relator index out of range"),
    }
}

/// The primitive sequence that undoes `witness`.
pub(crate) fn inverse_witness(witness: &[MoveId]) -> Vec<MoveId> {
    let inverse = inverse_table();
    witness.iter().rev().map(|&m| inverse[m as usize]).collect()
}

/// Conjugation words that move a different part of `word` to the front
/// without changing its content: suffixes of length 0..=k and inverses of
/// prefixes of length 1..=k. Applying one as a conjugation is a cyclic
/// rotation (up to free reduction).
fn rotation_words(word: &[i8], k: u8) -> Vec<Vec<i8>> {
    let k = k as usize;
    let mut out = vec![Vec::new()];
    for len in 1..=k.min(word.len()) {
        out.push(word[word.len() - len..].to_vec());
    }
    for len in 1..=k.min(word.len()) {
        out.push(word[..len].iter().rev().map(|letter| -letter).collect());
    }
    out
}

/// Primitive moves that conjugate `target` by `word`: applying conj(c1) then
/// conj(c2) conjugates by `c2*c1`, so the letters run in reverse order.
fn conjugation_witness(target: usize, word: &[i8]) -> Vec<MoveId> {
    word.iter()
        .rev()
        .map(|&letter| conjugation_move_id(target, letter))
        .collect()
}

/// Enumerate every witnessed cyclic-ring supermove for one exact state,
/// deduplicated by successor (best cancellation, then shortest witness) and
/// ordered deterministically: cancellation ratio desc, cancelled desc,
/// witness length asc, successor asc.
///
/// The family: rotate the target and/or the source by short words (suffixes
/// or inverse prefixes, so conjugation is a length-neutral cyclic rotation),
/// then multiply the rotated source in. The rotation is consumed by the
/// cancellation at the junction, so there is no un-rotation: rotating back
/// would re-add the letters the multiply already removed.
pub fn propose_all(pres: &Presentation, policy: &SupermovePolicy) -> Vec<Supermove> {
    let table: [Move; 14] =
        core::array::from_fn(|i| move_from_id(i as u8).expect("ids 0..=13 are pinned"));
    let base_len = pres.total_relator_length() as i64;
    let mut best: HashMap<Presentation, Supermove> = HashMap::new();
    for target in 0..2usize {
        let source = 1 - target;
        let target_words = rotation_words(&pres.relators[target], policy.max_conjugator);
        let source_words = rotation_words(&pres.relators[source], policy.max_conjugator);
        for target_rotation in &target_words {
            let conj_target = conjugation_witness(target, target_rotation);
            for source_rotation in &source_words {
                let conj_source = conjugation_witness(source, source_rotation);
                for invert_other in [false, true] {
                    let mut witness = Vec::with_capacity(conj_target.len() + conj_source.len() + 1);
                    witness.extend_from_slice(&conj_target);
                    witness.extend_from_slice(&conj_source);
                    witness.push(multiply_move_id(target, invert_other));
                    // The witness is the definition: replay it for the successor.
                    let mut state = pres.clone();
                    let mut ok = true;
                    for &id in &witness {
                        match apply_move(&state, &table[id as usize], DEFAULT_N_GEN) {
                            Ok(next) => state = next,
                            Err(_) => {
                                ok = false;
                                break;
                            }
                        }
                    }
                    if !ok || state == *pres {
                        continue;
                    }
                    let entry = Supermove {
                        cancelled: base_len - state.total_relator_length() as i64,
                        successor: state,
                        witness,
                    };
                    let replace = match best.get(&entry.successor) {
                        None => true,
                        Some(old) => {
                            entry.cancelled > old.cancelled
                                || (entry.cancelled == old.cancelled
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
    let mut all: Vec<Supermove> = best.into_values().collect();
    all.sort_by(|a, b| {
        (b.cancelled * a.witness.len() as i64)
            .cmp(&(a.cancelled * b.witness.len() as i64))
            .then_with(|| b.cancelled.cmp(&a.cancelled))
            .then_with(|| a.witness.len().cmp(&b.witness.len()))
            .then_with(|| a.successor.relators.cmp(&b.successor.relators))
    });
    all
}

/// Apply the proposal policy to the raw family: keep the best `top_k`
/// cancellation-producing proposals plus up to `keep_increasing`
/// length-non-decreasing proposals (setup moves).
pub fn propose(pres: &Presentation, policy: &SupermovePolicy) -> Vec<Supermove> {
    let all = propose_all(pres, policy);
    let mut chosen: Vec<Supermove> = Vec::new();
    for m in all.iter().filter(|m| m.cancelled > 0).take(policy.top_k) {
        chosen.push(m.clone());
    }
    for m in all
        .iter()
        .filter(|m| m.cancelled <= 0)
        .take(policy.keep_increasing)
    {
        chosen.push(m.clone());
    }
    chosen
}

/// Walk the supermove parent chain and flatten it into a primitive witness.
fn reconstruct_super(
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

/// Best-first supermove search: same frozen discipline as `greedy` (total
/// length, then discovery sequence; permanent exact-state suppression), with
/// witnessed supermoves added as additional successors and charged by
/// primitive witness length. Bank and ball probes match `atlas_search`.
pub(crate) fn solve_supermove_greedy(
    start: &Presentation,
    limits: &SearchLimits,
    policy: &SupermovePolicy,
    bank: &GoalBank,
    mut ball: Option<&mut ReverseBall>,
) -> AtlasOutcome {
    let done = |result, via_bank: bool, via_ball: bool, ball_hit_depth: Option<u32>| AtlasOutcome {
        result,
        via_bank,
        via_ball,
        ball_hit_depth,
    };
    let t0 = Instant::now();
    let root = match reduce_presentation(start) {
        Ok(root) => root,
        Err(msg) => {
            return done(
                failed(msg, SearchMetrics::default(), t0.elapsed()),
                false,
                false,
                None,
            );
        }
    };
    let mut metrics = SearchMetrics {
        max_total_relator_length: root.total_relator_length(),
        ..SearchMetrics::default()
    };
    if is_ac_target(&root) {
        metrics.unique = 1;
        return done(
            solved(&root, Vec::new(), metrics, t0.elapsed()),
            false,
            false,
            None,
        );
    }
    if let Some(suffix) = bank.suffix(&root) {
        metrics.unique = 1;
        return done(
            solved(&root, suffix.to_vec(), metrics, t0.elapsed()),
            true,
            false,
            None,
        );
    }
    let mut store = StateStore::new();
    store
        .insert_new(&root.relators[0], &root.relators[1], None, 0)
        .expect("fresh store accepts the root");
    // Supermove parent links, parallel to state ids: (parent id, witness slot).
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
    let table: [Move; 14] =
        core::array::from_fn(|i| move_from_id(i as u8).expect("ids 0..=13 are pinned"));
    let (mut w0, mut w1) = (Vec::new(), Vec::new());
    let mut pruned = false;
    while let Some(item) = heap.pop() {
        if limits
            .max_expanded
            .is_some_and(|cap| metrics.expanded >= cap)
        {
            return done(limit_reached(metrics, t0.elapsed()), false, false, None);
        }
        let depth = store.depth(item.id);
        // The exact inverse of the producing witness is a certain duplicate.
        let producer_inverse: Option<Vec<MoveId>> = parents[item.id as usize]
            .map(|(_, witness)| inverse_witness(&witnesses[witness as usize]));
        store.copy_state(item.id, &mut w0, &mut w1);
        let current = Presentation {
            relators: [std::mem::take(&mut w0), std::mem::take(&mut w1)],
        };
        // Primitive moves, each charged one unit.
        for id in all_move_ids() {
            if producer_inverse
                .as_deref()
                .is_some_and(|inv| inv.len() == 1 && inv[0] == id)
            {
                continue;
            }
            if limits.max_work.is_some_and(|cap| metrics.generated >= cap) {
                return done(limit_reached(metrics, t0.elapsed()), false, false, None);
            }
            let successor = match apply_move(&current, &table[id as usize], DEFAULT_N_GEN) {
                Ok(p) => p,
                Err(e) => return done(failed(e.0, metrics, t0.elapsed()), false, false, None),
            };
            metrics.generated += 1;
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
            witnesses.push(vec![id]);
            metrics.unique += 1;
            metrics.max_depth_seen = metrics.max_depth_seen.max(child_depth);
            if is_ac_target(&successor) {
                let certificate = reconstruct_super(&parents, &witnesses, child);
                return done(
                    solved(&root, certificate, metrics, t0.elapsed()),
                    false,
                    false,
                    None,
                );
            }
            let bank_len = bank.suffix(&successor).map(<[u8]>::len);
            let ball_hit: Option<(StateId, u32)> = match ball.as_mut() {
                Some(b) => b
                    .lookup(&successor.relators[0], &successor.relators[1])
                    .map(|hid| (hid, b.depth(hid))),
                None => None,
            };
            let use_ball = match (bank_len, ball_hit) {
                (Some(a), Some((_, b))) => (b as usize) <= a,
                (None, Some(_)) => true,
                _ => false,
            };
            if use_ball {
                let (hid, depth) = ball_hit.expect("selected ball path");
                let suffix = ball.as_ref().expect("ball probed above").suffix(hid);
                let mut certificate = reconstruct_super(&parents, &witnesses, child);
                certificate.extend_from_slice(&suffix);
                return done(
                    solved(&root, certificate, metrics, t0.elapsed()),
                    false,
                    true,
                    Some(depth),
                );
            }
            if let Some(suffix) = bank.suffix(&successor) {
                let mut certificate = reconstruct_super(&parents, &witnesses, child);
                certificate.extend_from_slice(suffix);
                return done(
                    solved(&root, certificate, metrics, t0.elapsed()),
                    true,
                    false,
                    None,
                );
            }
            heap.push(HeapItem {
                length,
                seq,
                id: child,
            });
            seq += 1;
            metrics.max_frontier = metrics.max_frontier.max(heap.len() as u64);
        }
        // Witnessed supermoves, charged by primitive witness length.
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
                return done(limit_reached(metrics, t0.elapsed()), false, false, None);
            }
            metrics.generated += cost;
            metrics.macros += 1;
            metrics.macro_witness_moves += cost;
            metrics.macro_cancelled += proposal.cancelled.max(0) as u64;
            let Supermove {
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
                let certificate = reconstruct_super(&parents, &witnesses, child);
                return done(
                    solved(&root, certificate, metrics, t0.elapsed()),
                    false,
                    false,
                    None,
                );
            }
            let bank_len = bank.suffix(&successor).map(<[u8]>::len);
            let ball_hit: Option<(StateId, u32)> = match ball.as_mut() {
                Some(b) => b
                    .lookup(&successor.relators[0], &successor.relators[1])
                    .map(|hid| (hid, b.depth(hid))),
                None => None,
            };
            let use_ball = match (bank_len, ball_hit) {
                (Some(a), Some((_, b))) => (b as usize) <= a,
                (None, Some(_)) => true,
                _ => false,
            };
            if use_ball {
                let (hid, depth) = ball_hit.expect("selected ball path");
                let suffix = ball.as_ref().expect("ball probed above").suffix(hid);
                let mut certificate = reconstruct_super(&parents, &witnesses, child);
                certificate.extend_from_slice(&suffix);
                return done(
                    solved(&root, certificate, metrics, t0.elapsed()),
                    false,
                    true,
                    Some(depth),
                );
            }
            if let Some(suffix) = bank.suffix(&successor) {
                let mut certificate = reconstruct_super(&parents, &witnesses, child);
                certificate.extend_from_slice(suffix);
                return done(
                    solved(&root, certificate, metrics, t0.elapsed()),
                    true,
                    false,
                    None,
                );
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
        done(limit_reached(metrics, t0.elapsed()), false, false, None)
    } else {
        done(exhausted(metrics, t0.elapsed()), false, false, None)
    }
}

#[cfg(test)]
mod supermove_tests {
    use super::*;
    use ac_core::replay_certificate;

    fn pres(r0: &[i8], r1: &[i8]) -> Presentation {
        Presentation {
            relators: [r0.to_vec(), r1.to_vec()],
        }
    }

    /// Apply a witness move by move (replay_certificate additionally requires
    /// the end state to be the exact target, which proposals need not be).
    fn apply_witness(start: &Presentation, witness: &[MoveId]) -> Presentation {
        let table: [Move; 14] =
            core::array::from_fn(|i| move_from_id(i as u8).expect("ids 0..=13 are pinned"));
        let mut state = start.clone();
        for &id in witness {
            state = apply_move(&state, &table[id as usize], DEFAULT_N_GEN)
                .expect("official moves apply to reduced states");
        }
        state
    }

    #[test]
    fn every_proposal_is_witnessed() {
        let states = [
            pres(&[1, 2], &[2]),
            pres(&[-1, -2], &[-1, -2, 1]),
            pres(&[1, -2, 1, 2, -1], &[2, 1, -2]),
            pres(&[-2, 1, 2], &[1, 2, -1]),
        ];
        for state in states {
            for policy in [
                SupermovePolicy::default(),
                SupermovePolicy {
                    max_conjugator: 2,
                    ..SupermovePolicy::default()
                },
                SupermovePolicy {
                    max_conjugator: 0,
                    ..SupermovePolicy::default()
                },
            ] {
                let proposals = propose(&state, &policy);
                for proposal in proposals {
                    assert!(!proposal.witness.is_empty());
                    assert_eq!(
                        apply_witness(&state, &proposal.witness),
                        proposal.successor,
                        "witness disagrees with successor"
                    );
                    assert_eq!(
                        proposal.cancelled,
                        state.total_relator_length() as i64
                            - proposal.successor.total_relator_length() as i64
                    );
                }
            }
        }
    }

    #[test]
    fn direct_append_proposals_include_known_cancellation() {
        // <x,y | xy, y>: multiplying r0 by r1^-1 (move 3) removes the letter
        // y from r0, one net letter.
        let proposals = propose(&pres(&[1, 2], &[2]), &SupermovePolicy::default());
        assert!(
            proposals.iter().any(|m| m.witness == vec![3]
                && m.cancelled == 1
                && m.successor == pres(&[1], &[2])),
            "missing direct [3] proposal: {proposals:?}"
        );
    }

    #[test]
    fn conjugator_length_is_respected_and_ranked() {
        let policy = SupermovePolicy {
            max_conjugator: 2,
            top_k: 4,
            keep_increasing: 2,
        };
        let proposals = propose(&pres(&[-1, -2], &[-1, -2, 1]), &policy);
        assert!(proposals.len() <= policy.top_k + policy.keep_increasing);
        let mut cancellations = proposals.iter().filter(|m| m.cancelled > 0);
        let first = cancellations.next();
        assert!(first.is_some(), "fixture should admit cancellations");
        // Cancelling proposals come first and never rank below later ones.
        let mut seen_non_cancelling = false;
        let mut prev_ratio = f64::INFINITY;
        for proposal in &proposals {
            if proposal.cancelled <= 0 {
                seen_non_cancelling = true;
                continue;
            }
            assert!(!seen_non_cancelling, "cancelling proposal after a filler");
            let ratio = proposal.cancelled as f64 / proposal.witness.len() as f64;
            assert!(ratio <= prev_ratio + 1e-12, "ratio order violated");
            prev_ratio = ratio;
            assert!(
                proposal.witness.len() <= 2 * policy.max_conjugator as usize + 1,
                "witness longer than rotations allow"
            );
        }
    }

    #[test]
    fn rotation_reaches_internal_cancellation() {
        // t = [2,2,1], s = [-2,-1]: no single primitive move cancels, but
        // conjugating t by -2 rotates the internal [2] into a junction match
        // and the multiply removes it: witness [9, 2], two net letters.
        let state = pres(&[2, 2, 1], &[-2, -1]);
        let proposals = propose(&state, &SupermovePolicy::default());
        assert!(
            proposals.iter().any(|m| m.cancelled == 2
                && m.witness.len() == 2
                && m.successor == pres(&[2], &[-2, -1])),
            "missing rotation proposal: {proposals:?}"
        );
    }

    #[test]
    fn solver_solves_and_charges_primitive_cost() {
        // back_4 with an empty bank: depth-4 optimum, certificate must replay.
        let limits = SearchLimits::default();
        let outcome = solve_supermove_greedy(
            &pres(&[-1, -2], &[-1, -2, 1]),
            &limits,
            &SupermovePolicy::default(),
            &GoalBank::new(),
            None,
        );
        assert!(matches!(outcome.result.status, crate::SearchStatus::Solved));
        assert!(outcome.result.metrics.replay_ok);
        let metrics = &outcome.result.metrics;
        assert!(metrics.macro_witness_moves <= metrics.generated);
        assert!(metrics.macros > 0, "fixture should emit supermoves");
        assert!(metrics.macro_witness_moves > 0);
        let cert = &outcome.result.certificate;
        let end = replay_certificate(&pres(&[-1, -2], &[-1, -2, 1]), cert).unwrap();
        assert!(is_ac_target(&end));
    }

    #[test]
    fn solver_is_deterministic() {
        let start = pres(&[-1, -2], &[-1, -2, 1]);
        let a = solve_supermove_greedy(
            &start,
            &SearchLimits::default(),
            &SupermovePolicy::default(),
            &GoalBank::new(),
            None,
        );
        let b = solve_supermove_greedy(
            &start,
            &SearchLimits::default(),
            &SupermovePolicy::default(),
            &GoalBank::new(),
            None,
        );
        assert_eq!(a.result.certificate, b.result.certificate);
        assert_eq!(a.result.metrics.generated, b.result.metrics.generated);
        assert_eq!(a.result.metrics.unique, b.result.metrics.unique);
        assert_eq!(a.result.metrics.macros, b.result.metrics.macros);
    }
}
