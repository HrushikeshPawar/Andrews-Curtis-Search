//! Solution atlas (S4.1): every intermediate exact state on every verified
//! certificate, mapped to its shortest known suffix to the exact target.
//!
//! Forward search then terminates on ANY banked state, not just `[[1],[2]]`:
//! reaching a banked state `s` completes the certificate with the stored
//! suffix. All entries are exact (replayed through ac-core at build time),
//! so bank hits need no further verification beyond the usual replay gate.
//!
//! v1 stores one suffix copy per state. Shared suffix tails duplicate
//! bytes; if the bank (or its predecessor-tube expansion) outgrows comfort,
//! replace values with `(next_state, next_move)` DAG links. Measure first.

use ac_core::{DEFAULT_N_GEN, MoveId, Presentation, apply_move, move_from_id, reduce};
use std::collections::HashMap;

#[derive(Default)]
pub struct GoalBank {
    table: HashMap<Presentation, Vec<MoveId>>,
}

impl GoalBank {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.table.len()
    }

    pub fn is_empty(&self) -> bool {
        self.table.is_empty()
    }

    /// Replay one certificate from `start`, inserting every intermediate
    /// state with its remaining suffix. Collisions keep the shorter suffix.
    /// The start is freely reduced first, exactly like `replay_certificate`,
    /// so banked states align with solver states. Move ids are validated
    /// before anything is inserted, so a bad certificate leaves no trace.
    pub fn insert_certificate(
        &mut self,
        start: &Presentation,
        cert: &[MoveId],
    ) -> Result<(), String> {
        let mut state = Presentation {
            relators: [
                reduce(&start.relators[0], DEFAULT_N_GEN).map_err(|e| e.0)?,
                reduce(&start.relators[1], DEFAULT_N_GEN).map_err(|e| e.0)?,
            ],
        };
        let mut moves = Vec::with_capacity(cert.len());
        for &id in cert {
            moves.push(move_from_id(id).map_err(|e| e.0)?);
        }
        for (i, mv) in moves.iter().enumerate() {
            let suffix = &cert[i..];
            match self.table.get(&state) {
                Some(known) if known.len() <= suffix.len() => {}
                _ => {
                    self.table.insert(state.clone(), suffix.to_vec());
                }
            }
            state = apply_move(&state, mv, DEFAULT_N_GEN).map_err(|e| e.0)?;
        }
        // The final state reaches the target in zero moves.
        match self.table.get(&state) {
            Some(known) if known.is_empty() => {}
            _ => {
                self.table.insert(state, Vec::new());
            }
        }
        Ok(())
    }

    /// Shortest known suffix from `state` to the exact target, if banked.
    pub fn suffix(&self, state: &Presentation) -> Option<&[MoveId]> {
        self.table.get(state).map(Vec::as_slice)
    }

    /// Iterate every banked state with its shortest known suffix (analysis
    /// and build statistics only; traversal never iterates the bank).
    pub fn iter(&self) -> impl Iterator<Item = (&Presentation, &[MoveId])> {
        self.table
            .iter()
            .map(|(state, suffix)| (state, suffix.as_slice()))
    }

    /// Suffix lengths over all banked states (build statistics).
    pub fn suffix_lengths(&self) -> impl Iterator<Item = usize> + '_ {
        self.table.values().map(Vec::len)
    }
}

#[cfg(test)]
mod goalbank_tests {
    use super::*;
    use ac_core::is_ac_target;

    fn pres(r0: &[i8], r1: &[i8]) -> Presentation {
        Presentation {
            relators: [r0.to_vec(), r1.to_vec()],
        }
    }

    #[test]
    fn single_certificate_banks_every_intermediate() {
        // xy_y via [3]: start keeps suffix [3], target keeps [].
        let mut bank = GoalBank::new();
        bank.insert_certificate(&pres(&[1, 2], &[2]), &[3]).unwrap();
        assert_eq!(bank.len(), 2);
        assert_eq!(bank.suffix(&pres(&[1, 2], &[2])), Some(&[3][..]));
        assert_eq!(bank.suffix(&pres(&[1], &[2])), Some(&[][..]));
        assert_eq!(bank.suffix(&pres(&[2], &[1])), None);
    }

    #[test]
    fn collisions_keep_the_shorter_suffix() {
        // [0,0,3] revisits its start (invert is an involution): the start
        // must keep suffix [3], not the longer [0,0,3]; the detour state is
        // still banked with its own suffix.
        let mut bank = GoalBank::new();
        bank.insert_certificate(&pres(&[1, 2], &[2]), &[3]).unwrap();
        bank.insert_certificate(&pres(&[1, 2], &[2]), &[0, 0, 3])
            .unwrap();
        assert_eq!(bank.suffix(&pres(&[1, 2], &[2])), Some(&[3][..]));
        assert_eq!(bank.len(), 3);
        let detour = bank.suffix(&pres(&[-2, -1], &[2]));
        assert_eq!(detour, Some(&[0, 3][..]));
    }

    #[test]
    fn every_banked_suffix_replays_to_the_target() {
        use ac_core::{DEFAULT_N_GEN, apply_move, move_from_id};
        let mut bank = GoalBank::new();
        let start = pres(&[-1, -2], &[-1, -2, 1]);
        bank.insert_certificate(&start, &[0, 1, 3, 10]).unwrap();
        // 4 intermediates plus the target itself.
        assert_eq!(bank.len(), 5);
        let mut state = start;
        for remaining in [4, 3, 2, 1, 0] {
            let suffix = bank.suffix(&state).expect("banked");
            assert_eq!(suffix.len(), remaining);
            let end = ac_core::replay_certificate(&state, suffix).unwrap();
            assert!(is_ac_target(&end));
            if remaining > 0 {
                let mv = move_from_id(suffix[0]).unwrap();
                state = apply_move(&state, &mv, DEFAULT_N_GEN).unwrap();
            }
        }
    }

    #[test]
    fn bad_move_id_fails_honestly() {
        let mut bank = GoalBank::new();
        assert!(
            bank.insert_certificate(&pres(&[1, 2], &[2]), &[14])
                .is_err()
        );
        assert!(bank.is_empty());
    }
}
