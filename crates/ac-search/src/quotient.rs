//! S4.2a quotient instrumentation: class-2 nilpotent and abelian images of
//! the exact state graph, with exhaustive abstract distance tables.
//!
//! The class-2 quotient is `H = F2 / (gamma_3(F2) * F2^p)`, the free class-2
//! nilpotent group of exponent `p` on two generators. Elements have unique
//! normal forms `x^a y^b [x,y]^c` with `a, b, c` in `Z/p` (order `p^3`); a
//! presentation maps to its pair of relator images in `H x H` (order `p^6`).
//!
//! Every official move projects to an abstract move on that signature
//! (`successor` mirrors the 14 rows of `ac-core::moves` exactly), so the
//! projection is a graph homomorphism and the abstract distance to the
//! abstract target is a genuine lower bound on exact primitive distance.
//! The abelian quotient `H / [H,H]` is the same construction with the
//! commutator coordinate dropped and conjugation collapsed to the identity.
//!
//! This module is analysis-only: it contains no search.

use ac_core::Presentation;
use std::collections::VecDeque;

/// A class-2 element `x^a y^b [x,y]^c` with coordinates in `Z/p`.
pub type Class2Element = [u8; 3];
/// The pair of relator images `(e0, e1)` as `[a0, b0, c0, a1, b1, c1]`.
pub type Class2Signature = [u8; 6];
/// Abelianized relator pair: total `x`/`y` exponents of each relator.
pub type AbelianSignature = [u8; 4];

/// Distances that no abstract path reaches (e.g. off-image states).
pub const UNREACHABLE: u16 = u16::MAX;

fn add_mod(a: u8, b: u8, p: u8) -> u8 {
    ((a as u16 + b as u16) % p as u16) as u8
}

fn mul_mod(a: u8, b: u8, p: u8) -> u8 {
    ((a as u16 * b as u16) % p as u16) as u8
}

fn neg_mod(a: u8, p: u8) -> u8 {
    if a == 0 { 0 } else { p - a }
}

fn check_modulus(p: u8) {
    assert!(p >= 2, "quotient modulus must be at least 2");
}

/// Distance-to-source table over one abstract state space (multi-source BFS).
pub struct DistanceTable {
    dist: Vec<u16>,
}

impl DistanceTable {
    pub fn space(&self) -> usize {
        self.dist.len()
    }

    /// `UNREACHABLE` when no source connects to this abstract state.
    pub fn get(&self, idx: usize) -> u16 {
        self.dist[idx]
    }
}

fn bfs(space: usize, sources: &[usize], neighbors: impl Fn(usize) -> [usize; 14]) -> DistanceTable {
    let mut dist = vec![UNREACHABLE; space];
    let mut queue: VecDeque<usize> = VecDeque::new();
    for &s in sources {
        assert!(s < space, "abstract source out of range");
        if dist[s] == UNREACHABLE {
            dist[s] = 0;
            queue.push_back(s);
        }
    }
    while let Some(u) = queue.pop_front() {
        let next = dist[u] + 1;
        for v in neighbors(u) {
            if dist[v] == UNREACHABLE {
                dist[v] = next;
                queue.push_back(v);
            }
        }
    }
    DistanceTable { dist }
}

/// Class-2 nilpotent quotient `F2 / (gamma_3(F2) * F2^p)`.
pub struct Class2Quotient {
    p: u8,
}

impl Class2Quotient {
    pub fn new(p: u8) -> Self {
        check_modulus(p);
        Self { p }
    }

    pub fn p(&self) -> u8 {
        self.p
    }

    /// Abstract state count `p^6` (two relator images in `H`).
    pub fn space(&self) -> usize {
        (self.p as usize).pow(6)
    }

    pub fn identity(&self) -> Class2Element {
        [0, 0, 0]
    }

    /// `x^a1 y^b1 z^c1 * x^a2 y^b2 z^c2 = x^(a1+a2) y^(b1+b2) z^(c1+c2-a2*b1)`.
    pub fn multiply(&self, x: Class2Element, y: Class2Element) -> Class2Element {
        let p = self.p;
        [
            add_mod(x[0], y[0], p),
            add_mod(x[1], y[1], p),
            add_mod(
                add_mod(x[2], y[2], p),
                neg_mod(mul_mod(y[0], x[1], p), p),
                p,
            ),
        ]
    }

    /// `(x^a y^b z^c)^-1 = x^-a y^-b z^(-c-a*b)`.
    pub fn inverse(&self, x: Class2Element) -> Class2Element {
        let p = self.p;
        [
            neg_mod(x[0], p),
            neg_mod(x[1], p),
            neg_mod(add_mod(x[2], mul_mod(x[0], x[1], p), p), p),
        ]
    }

    /// Conjugation `g x g^-1` by a single generator letter `g` in `{+1,-1,+2,-2}`:
    /// by `x` the commutator exponent shifts by `b`, by `y` by `-a`.
    pub fn conjugate(&self, x: Class2Element, letter: i8) -> Class2Element {
        let p = self.p;
        let delta = match letter {
            1 => x[1],
            -1 => neg_mod(x[1], p),
            2 => neg_mod(x[0], p),
            -2 => x[0],
            other => panic!("conjugator {other} is not one of ±1, ±2"),
        };
        [x[0], x[1], add_mod(x[2], delta, p)]
    }

    /// Project a freely reduced word: right-multiply each generator image in order.
    pub fn project_word(&self, word: &[i8]) -> Class2Element {
        let mut e = self.identity();
        for &letter in word {
            let g = (letter.unsigned_abs() - 1) as usize;
            let mut image = self.identity();
            image[g] = 1;
            if letter < 0 {
                image = self.inverse(image);
            }
            e = self.multiply(e, image);
        }
        e
    }

    /// Project both relators of a presentation into `H x H`.
    pub fn project(&self, pres: &Presentation) -> Class2Signature {
        let r0 = self.project_word(&pres.relators[0]);
        let r1 = self.project_word(&pres.relators[1]);
        [r0[0], r0[1], r0[2], r1[0], r1[1], r1[2]]
    }

    pub fn index(&self, sig: &Class2Signature) -> usize {
        let p = self.p as usize;
        let mut idx = 0usize;
        for &coord in sig {
            debug_assert!((coord as usize) < p, "signature coordinate out of range");
            idx = idx * p + coord as usize;
        }
        idx
    }

    fn successor_index(&self, idx: usize) -> [usize; 14] {
        let p = self.p as usize;
        let mut coords = [0u8; 6];
        let mut rest = idx;
        for slot in coords.iter_mut().rev() {
            *slot = (rest % p) as u8;
            rest /= p;
        }
        let e0 = [coords[0], coords[1], coords[2]];
        let e1 = [coords[3], coords[4], coords[5]];
        let pair = |a: Class2Element, b: Class2Element| [a[0], a[1], a[2], b[0], b[1], b[2]];
        let conj = |e: Class2Element, letter: i8| self.conjugate(e, letter);
        let successors: [Class2Signature; 14] = [
            pair(self.inverse(e0), e1),
            pair(e0, self.inverse(e1)),
            pair(self.multiply(e0, e1), e1),
            pair(self.multiply(e0, self.inverse(e1)), e1),
            pair(e0, self.multiply(e1, e0)),
            pair(e0, self.multiply(e1, self.inverse(e0))),
            pair(conj(e0, 1), e1),
            pair(conj(e0, -1), e1),
            pair(conj(e0, 2), e1),
            pair(conj(e0, -2), e1),
            pair(e0, conj(e1, 1)),
            pair(e0, conj(e1, -1)),
            pair(e0, conj(e1, 2)),
            pair(e0, conj(e1, -2)),
        ];
        successors.map(|sig| self.index(&sig))
    }

    /// Abstract successor signature for one official move id (0..=13).
    pub fn successor(&self, sig: &Class2Signature, move_id: u8) -> Class2Signature {
        assert!(move_id < 14, "move id out of range");
        let idx = self.index(sig);
        let next = self.successor_index(idx)[move_id as usize];
        let mut out = [0u8; 6];
        let p = self.p as usize;
        let mut rest = next;
        for slot in out.iter_mut().rev() {
            *slot = (rest % p) as u8;
            rest /= p;
        }
        out
    }

    /// Exhaustive abstract distances to the image of `[[1],[2]]`.
    pub fn distance_from_target(&self) -> DistanceTable {
        self.distance_from_sources(&[self.project(&Presentation::target())])
    }

    /// Exhaustive multi-source abstract distances to the given signatures.
    pub fn distance_from_sources(&self, sources: &[Class2Signature]) -> DistanceTable {
        let idx: Vec<usize> = sources.iter().map(|s| self.index(s)).collect();
        bfs(self.space(), &idx, |u| self.successor_index(u))
    }
}

/// Abelianization `F2 / [F2,F2]` reduced mod `p`.
pub struct AbelianQuotient {
    p: u8,
}

impl AbelianQuotient {
    pub fn new(p: u8) -> Self {
        check_modulus(p);
        Self { p }
    }

    pub fn p(&self) -> u8 {
        self.p
    }

    /// Abstract state count `p^4` (two relator images in `Z/p x Z/p`).
    pub fn space(&self) -> usize {
        (self.p as usize).pow(4)
    }

    pub fn project_word(&self, word: &[i8]) -> [u8; 2] {
        let p = self.p;
        let mut e = [0u8, 0u8];
        for &letter in word {
            let g = (letter.unsigned_abs() - 1) as usize;
            let delta = if letter < 0 { p - 1 } else { 1 };
            e[g] = add_mod(e[g], delta, p);
        }
        e
    }

    pub fn project(&self, pres: &Presentation) -> AbelianSignature {
        let r0 = self.project_word(&pres.relators[0]);
        let r1 = self.project_word(&pres.relators[1]);
        [r0[0], r0[1], r1[0], r1[1]]
    }

    pub fn index(&self, sig: &AbelianSignature) -> usize {
        let p = self.p as usize;
        let mut idx = 0usize;
        for &coord in sig {
            debug_assert!((coord as usize) < p, "signature coordinate out of range");
            idx = idx * p + coord as usize;
        }
        idx
    }

    fn successor_index(&self, idx: usize) -> [usize; 14] {
        let p = self.p as usize;
        let mut coords = [0u8; 4];
        let mut rest = idx;
        for slot in coords.iter_mut().rev() {
            *slot = (rest % p) as u8;
            rest /= p;
        }
        let e0 = [coords[0], coords[1]];
        let e1 = [coords[2], coords[3]];
        let pair = |a: [u8; 2], b: [u8; 2]| [a[0], a[1], b[0], b[1]];
        let neg = |e: [u8; 2]| [neg_mod(e[0], self.p), neg_mod(e[1], self.p)];
        let add =
            |x: [u8; 2], y: [u8; 2]| [add_mod(x[0], y[0], self.p), add_mod(x[1], y[1], self.p)];
        // Conjugation is the identity after abelianization: moves 6..=13
        // return the state unchanged.
        let successors: [AbelianSignature; 14] = [
            pair(neg(e0), e1),
            pair(e0, neg(e1)),
            pair(add(e0, e1), e1),
            pair(add(e0, neg(e1)), e1),
            pair(e0, add(e1, e0)),
            pair(e0, add(e1, neg(e0))),
            pair(e0, e1),
            pair(e0, e1),
            pair(e0, e1),
            pair(e0, e1),
            pair(e0, e1),
            pair(e0, e1),
            pair(e0, e1),
            pair(e0, e1),
        ];
        successors.map(|sig| self.index(&sig))
    }

    /// Abstract successor signature for one official move id (0..=13).
    pub fn successor(&self, sig: &AbelianSignature, move_id: u8) -> AbelianSignature {
        assert!(move_id < 14, "move id out of range");
        let idx = self.index(sig);
        let next = self.successor_index(idx)[move_id as usize];
        let mut out = [0u8; 4];
        let p = self.p as usize;
        let mut rest = next;
        for slot in out.iter_mut().rev() {
            *slot = (rest % p) as u8;
            rest /= p;
        }
        out
    }

    pub fn distance_from_target(&self) -> DistanceTable {
        self.distance_from_sources(&[self.project(&Presentation::target())])
    }

    pub fn distance_from_sources(&self, sources: &[AbelianSignature]) -> DistanceTable {
        let idx: Vec<usize> = sources.iter().map(|s| self.index(s)).collect();
        bfs(self.space(), &idx, |u| self.successor_index(u))
    }
}

#[cfg(test)]
mod quotient_tests {
    use super::*;
    use ac_core::{
        DEFAULT_N_GEN, all_move_ids, apply_move, concat, conjugate, inverse, move_from_id, reduce,
    };

    fn pres(r0: &[i8], r1: &[i8]) -> Presentation {
        Presentation {
            relators: [r0.to_vec(), r1.to_vec()],
        }
    }

    /// Deterministic pseudo-random reduced states (xorshift like store tests).
    fn random_presentations(count: usize) -> Vec<Presentation> {
        let mut rng: u64 = 0x9E3779B97F4A7C15;
        let mut out = Vec::with_capacity(count);
        for i in 0..count {
            let n0 = (rng % 11) as usize + i % 3;
            let r0: Vec<i8> = (0..n0).map(|_| next_letter(&mut rng)).collect();
            let n1 = (rng % 7) as usize + i % 5;
            let r1: Vec<i8> = (0..n1).map(|_| next_letter(&mut rng)).collect();
            out.push(pres(
                &reduce(&r0, DEFAULT_N_GEN).unwrap(),
                &reduce(&r1, DEFAULT_N_GEN).unwrap(),
            ));
        }
        out
    }

    fn next_letter(rng: &mut u64) -> i8 {
        *rng ^= *rng << 13;
        *rng ^= *rng >> 7;
        *rng ^= *rng << 17;
        [1, -1, 2, -2][(*rng % 4) as usize]
    }

    #[test]
    fn space_sizes_are_p_pow_6_and_p_pow_4() {
        let c2 = Class2Quotient::new(5);
        assert_eq!(c2.space(), 15_625);
        assert_eq!(AbelianQuotient::new(5).space(), 625);
    }

    #[test]
    fn projection_is_a_homomorphism() {
        for p in [2u8, 3, 5] {
            let q = Class2Quotient::new(p);
            let ab = AbelianQuotient::new(p);
            for state in random_presentations(40) {
                for (a, b) in [
                    (state.relators[0].clone(), state.relators[1].clone()),
                    (state.relators[1].clone(), state.relators[0].clone()),
                ] {
                    let joined = concat(&a, &b, DEFAULT_N_GEN).unwrap();
                    // Multiplicative law, inverse law, conjugation law.
                    assert_eq!(
                        q.project_word(&joined),
                        q.multiply(q.project_word(&a), q.project_word(&b))
                    );
                    assert_eq!(
                        q.project_word(&inverse(&a, DEFAULT_N_GEN).unwrap()),
                        q.inverse(q.project_word(&a))
                    );
                    for letter in [1i8, -1, 2, -2] {
                        let c = conjugate(&a, letter, DEFAULT_N_GEN).unwrap();
                        assert_eq!(q.project_word(&c), q.conjugate(q.project_word(&a), letter));
                    }
                    // Abelian mirror: only exponent sums survive, so this is
                    // addition mod p and conjugation fixes every element.
                    let sum =
                        |x: [u8; 2], y: [u8; 2]| [add_mod(x[0], y[0], p), add_mod(x[1], y[1], p)];
                    let neg = |x: [u8; 2]| [neg_mod(x[0], p), neg_mod(x[1], p)];
                    assert_eq!(
                        ab.project_word(&joined),
                        sum(ab.project_word(&a), ab.project_word(&b))
                    );
                    assert_eq!(
                        ab.project_word(&inverse(&a, DEFAULT_N_GEN).unwrap()),
                        neg(ab.project_word(&a))
                    );
                    for letter in [1i8, -1, 2, -2] {
                        let c = conjugate(&a, letter, DEFAULT_N_GEN).unwrap();
                        assert_eq!(ab.project_word(&c), ab.project_word(&a));
                    }
                }
            }
        }
    }

    #[test]
    fn projection_commutes_with_every_official_move() {
        for p in [2u8, 3, 5] {
            let q = Class2Quotient::new(p);
            let ab = AbelianQuotient::new(p);
            for state in random_presentations(12) {
                for id in all_move_ids() {
                    let mv = move_from_id(id).unwrap();
                    let next = apply_move(&state, &mv, DEFAULT_N_GEN).unwrap();
                    let c2_before = q.project(&state);
                    assert_eq!(
                        q.project(&next),
                        q.successor(&c2_before, id),
                        "class-2 projection of move {id} does not commute"
                    );
                    let ab_before = ab.project(&state);
                    assert_eq!(
                        ab.project(&next),
                        ab.successor(&ab_before, id),
                        "abelian projection of move {id} does not commute"
                    );
                }
            }
        }
    }

    #[test]
    fn target_projects_to_distance_zero() {
        for p in [2u8, 3, 5] {
            let q = Class2Quotient::new(p);
            let ab = AbelianQuotient::new(p);
            let sig = q.project(&Presentation::target());
            assert_eq!(q.distance_from_target().get(q.index(&sig)), 0);
            let absig = ab.project(&Presentation::target());
            assert_eq!(ab.distance_from_target().get(ab.index(&absig)), 0);
        }
    }

    #[test]
    fn distances_are_lipschitz_on_every_edge() {
        let q = Class2Quotient::new(3);
        let table = q.distance_from_target();
        for idx in 0..q.space() {
            let d = table.get(idx);
            for v in q.successor_index(idx) {
                let dv = table.get(v);
                let diff = d.abs_diff(dv);
                assert!(diff <= 1, "edge {idx}->{v} changes distance by {diff}");
            }
        }
    }

    #[test]
    fn abelian_distance_never_exceeds_class2_distance() {
        // Abelianization is a further quotient: every class-2 path projects
        // to an abelian path, so its optimum can only be smaller.
        let p = 5u8;
        let q = Class2Quotient::new(p);
        let ab = AbelianQuotient::new(p);
        let c2_table = q.distance_from_target();
        let ab_table = ab.distance_from_target();
        for a0 in 0..p {
            for b0 in 0..p {
                for c0 in 0..p {
                    for a1 in 0..p {
                        for b1 in 0..p {
                            for c1 in 0..p {
                                let sig = [a0, b0, c0, a1, b1, c1];
                                let ab_sig = [a0, b0, a1, b1];
                                assert!(
                                    ab_table.get(ab.index(&ab_sig)) <= c2_table.get(q.index(&sig)),
                                    "abelian bound violated at {sig:?}"
                                );
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn ball_one_states_are_within_abstract_distance_one() {
        // Every official successor of the target is at exact distance <= 1,
        // so its class-2 image must be at abstract distance <= 1.
        let q = Class2Quotient::new(5);
        let table = q.distance_from_target();
        let target = Presentation::target();
        for id in all_move_ids() {
            let mv = move_from_id(id).unwrap();
            let next = apply_move(&target, &mv, DEFAULT_N_GEN).unwrap();
            let d = table.get(q.index(&q.project(&next)));
            assert!(
                d <= 1,
                "ball-1 state reached by move {id} has abstract distance {d}"
            );
        }
    }
}
