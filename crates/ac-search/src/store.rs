//! C2 retained-state storage (S2.1): one copy of every discovered state.
//!
//! Layout per state in the append-only bump arena:
//! `[len0:u32 LE][len1:u32 LE][r0 bytes][r1 bytes]`, one byte per letter
//! (`(letter + 2) as u8`, injective on valid letters, so packed-byte
//! equality <=> exact-presentation equality). Chunks are fixed-size boxes:
//! record addresses never move, and records are never mutated.
//!
//! Everything else references a state by `StateId` (discovery order):
//! arena handles, first-discovery parents, depths, and the visited set —
//! a `hashbrown::HashTable<StateId>` whose hash/equality read through the
//! arena. The table is membership-only; traversal order never depends on it.
//!
//! The store only ever receives freely reduced, valid words (the BFS root is
//! reduced on entry; successors come from `apply_move`), so packing performs
//! no validation on the hot path.

use super::Parent;
use hashbrown::HashTable;
use std::collections::hash_map::RandomState;
use std::hash::BuildHasher;

pub(crate) type StateId = u32;

/// Bump chunk size. Records are small (tens of bytes at these depths), so one
/// chunk holds millions of states and addresses stay stable across appends.
const CHUNK_LEN: usize = 64 << 20;

pub(crate) struct StateStore {
    chunks: Vec<Box<[u8]>>,
    used: usize,
    handles: Vec<u64>,
    parents: Vec<Option<Parent>>,
    depths: Vec<u32>,
    visited: HashTable<StateId>,
    build: RandomState,
    scratch: Vec<u8>,
}

fn pack_into(buf: &mut Vec<u8>, r0: &[i8], r1: &[i8]) {
    buf.clear();
    buf.extend_from_slice(&(r0.len() as u32).to_le_bytes());
    buf.extend_from_slice(&(r1.len() as u32).to_le_bytes());
    for &l in r0.iter().chain(r1.iter()) {
        debug_assert!(l != 0 && l.unsigned_abs() <= 2, "store holds valid words only");
        buf.push((l + 2) as u8);
    }
}

fn decode_into(d0: &mut Vec<i8>, d1: &mut Vec<i8>, record: &[u8]) {
    let n0 = u32::from_le_bytes(record[0..4].try_into().expect("record header")) as usize;
    let n1 = u32::from_le_bytes(record[4..8].try_into().expect("record header")) as usize;
    d0.clear();
    d1.clear();
    d0.extend(record[8..8 + n0].iter().map(|&b| b as i8 - 2));
    d1.extend(record[8 + n0..8 + n0 + n1].iter().map(|&b| b as i8 - 2));
}

fn record_of(chunks: &[Box<[u8]>], handle: u64) -> &[u8] {
    let chunk = (handle >> 32) as usize;
    let off = (handle & 0xffff_ffff) as usize;
    let n0 = u32::from_le_bytes(chunks[chunk][off..off + 4].try_into().expect("record")) as usize;
    let n1 =
        u32::from_le_bytes(chunks[chunk][off + 4..off + 8].try_into().expect("record")) as usize;
    &chunks[chunk][off..off + 8 + n0 + n1]
}

fn hash_bytes(build: &RandomState, bytes: &[u8]) -> u64 {
    build.hash_one(bytes)
}

impl StateStore {
    pub(crate) fn new() -> Self {
        Self {
            chunks: Vec::new(),
            used: 0,
            handles: Vec::new(),
            parents: Vec::new(),
            depths: Vec::new(),
            visited: HashTable::new(),
            build: RandomState::new(),
            scratch: Vec::new(),
        }
    }

    /// Distinct states stored (== next discovery id). Test observability.
    #[cfg(test)]
    pub(crate) fn len(&self) -> u32 {
        self.handles.len() as u32
    }

    fn append(&mut self, bytes: &[u8]) -> u64 {
        assert!(bytes.len() <= CHUNK_LEN, "record exceeds chunk size");
        if self.chunks.is_empty() || self.used + bytes.len() > self.chunks.last().expect("chunk").len() {
            self.chunks.push(vec![0u8; CHUNK_LEN].into_boxed_slice());
            self.used = 0;
        }
        let idx = self.chunks.len() - 1;
        let off = self.used;
        self.chunks[idx][off..off + bytes.len()].copy_from_slice(bytes);
        self.used += bytes.len();
        ((idx as u64) << 32) | off as u64
    }

    /// Membership probe without inserting. Packs the candidate into the
    /// scratch buffer and tests it against the visited table.
    pub(crate) fn contains(&mut self, r0: &[i8], r1: &[i8]) -> bool {
        pack_into(&mut self.scratch, r0, r1);
        let hash = hash_bytes(&self.build, &self.scratch);
        self.visited
            .find(hash, |&id| {
                record_of(&self.chunks, self.handles[id as usize]) == self.scratch.as_slice()
            })
            .is_some()
    }

    /// First-visit-wins insert. `None` if the exact state is already stored
    /// (the recorded parent is never overwritten); otherwise appends one copy
    /// and returns the new `StateId`.
    pub(crate) fn insert_new(
        &mut self,
        r0: &[i8],
        r1: &[i8],
        parent: Option<Parent>,
        depth: u32,
    ) -> Option<StateId> {
        pack_into(&mut self.scratch, r0, r1);
        let hash = hash_bytes(&self.build, &self.scratch);
        let present = self.visited.find(hash, |&id| {
            record_of(&self.chunks, self.handles[id as usize]) == self.scratch.as_slice()
        });
        if present.is_some() {
            return None;
        }
        let bytes = std::mem::take(&mut self.scratch);
        let handle = self.append(&bytes);
        self.scratch = bytes;
        self.scratch.clear();
        let id = self.handles.len() as StateId;
        self.handles.push(handle);
        self.parents.push(parent);
        self.depths.push(depth);
        // Rehash closure for table growth: hashes the stored record with the
        // same builder, so every id keeps a consistent hash.
        let build = self.build.clone();
        let chunks = &self.chunks;
        let handles = &self.handles;
        self.visited.insert_unique(hash, id, |&sid| {
            hash_bytes(&build, record_of(chunks, handles[sid as usize]))
        });
        Some(id)
    }

    /// Decode a state into caller-owned buffers (capacity reused across
    /// calls). The hot expansion path: no allocation per popped state.
    pub(crate) fn copy_state(&self, id: StateId, out0: &mut Vec<i8>, out1: &mut Vec<i8>) {
        decode_into(out0, out1, record_of(&self.chunks, self.handles[id as usize]));
    }

    pub(crate) fn parent(&self, id: StateId) -> Option<Parent> {
        self.parents[id as usize]
    }

    pub(crate) fn depth(&self, id: StateId) -> u32 {
        self.depths[id as usize]
    }
}

#[cfg(test)]
mod store_tests {
    use super::*;

    fn insert(store: &mut StateStore, r0: &[i8], r1: &[i8]) -> Option<StateId> {
        let depth = store.len();
        store.insert_new(r0, r1, None, depth)
    }

    #[test]
    fn codec_round_trips_including_empty_words() {
        let cases: &[(&[i8], &[i8])] = &[
            (&[], &[]),
            (&[1], &[2]),
            (&[-2, 1, 2], &[1, 2, -1]),
            (&[-1, 2, 1, -2, -2], &[-1, 2]),
            (&[-2; 300], &[1, -1, 2, -2, 1]),
        ];
        for (r0, r1) in cases {
            let mut buf = Vec::new();
            pack_into(&mut buf, r0, r1);
            let (mut d0, mut d1) = (Vec::new(), Vec::new());
            decode_into(&mut d0, &mut d1, &buf);
            assert_eq!(d0, *r0);
            assert_eq!(d1, *r1);
        }
    }

    #[test]
    fn packing_distinguishes_order_and_content() {
        // Byte equality must mirror exact-presentation equality: order
        // matters, and no two distinct test states share packed bytes.
        let states: &[(&[i8], &[i8])] = &[
            (&[1], &[2]),
            (&[2], &[1]),
            (&[1, 2], &[2]),
            (&[1], &[2, 1]),
            (&[1, -1, 2], &[2]),
        ];
        let mut packed = Vec::new();
        for (i, (r0, r1)) in states.iter().enumerate() {
            let mut buf = Vec::new();
            pack_into(&mut buf, r0, r1);
            for (j, prev) in packed.iter().enumerate() {
                assert_ne!(&buf, prev, "states {i} and {j} collide");
            }
            packed.push(buf);
        }
    }

    #[test]
    fn first_visit_wins_and_state_round_trips() {
        let mut store = StateStore::new();
        assert_eq!(insert(&mut store, &[1], &[2]), Some(0));
        assert_eq!(store.len(), 1);
        // Duplicate: no new id, nothing overwritten.
        assert_eq!(insert(&mut store, &[1], &[2]), None);
        assert_eq!(store.len(), 1);
        assert_eq!(insert(&mut store, &[2], &[1]), Some(1));
        assert_eq!(store.len(), 2);
        let (mut o0, mut o1) = (Vec::new(), Vec::new());
        store.copy_state(0, &mut o0, &mut o1);
        assert_eq!((&o0, &o1), (&vec![1], &vec![2]));
        store.copy_state(1, &mut o0, &mut o1);
        assert_eq!((&o0, &o1), (&vec![2], &vec![1]));
    }

    #[test]
    fn parents_and_depths_are_recorded() {
        let mut store = StateStore::new();
        let root = store
            .insert_new(&[1, 2], &[2], None, 0)
            .expect("root is new");
        assert_eq!(store.parent(root), None);
        assert_eq!(store.depth(root), 0);
        let child = store
            .insert_new(
                &[1],
                &[2],
                Some(Parent {
                    parent_index: root,
                    move_id: 3,
                }),
                1,
            )
            .expect("child is new");
        assert_eq!(
            store.parent(child),
            Some(Parent {
                parent_index: root,
                move_id: 3,
            })
        );
        assert_eq!(store.depth(child), 1);
    }

    #[test]
    fn copy_state_decodes_into_caller_buffers() {
        let mut store = StateStore::new();
        insert(&mut store, &[-1, 2, 1, -2, -2], &[-1, 2]);
        insert(&mut store, &[1], &[2]);
        let (mut o0, mut o1) = (Vec::new(), Vec::new());
        store.copy_state(0, &mut o0, &mut o1);
        assert_eq!((&o0, &o1), (&vec![-1, 2, 1, -2, -2], &vec![-1, 2]));
        let cap = (o0.capacity(), o1.capacity());
        store.copy_state(1, &mut o0, &mut o1);
        assert_eq!((&o0, &o1), (&vec![1], &vec![2]));
        // Buffers are reused, not reallocated, across calls.
        assert!(o0.capacity() >= cap.0 && o1.capacity() >= cap.1);
    }

    #[test]
    fn bulk_insert_counts_distinct_states_across_chunks() {
        // Deterministic pseudo-random words (xorshift); small chunk path is
        // exercised through count, not chunk size (64 MB chunks hold this).
        let mut store = StateStore::new();
        let mut rng: u64 = 0x9E3779B97F4A7C15;
        let mut distinct = 0u32;
        let mut seen = std::collections::HashSet::new();
        for i in 0..20_000u32 {
            rng ^= rng << 13;
            rng ^= rng >> 7;
            rng ^= rng << 17;
            let r0: Vec<i8> = (0..(rng % 8) as usize)
                .map(|k| [1, -1, 2, -2][(rng.wrapping_add(k as u64 + i as u64) % 4) as usize])
                .collect();
            let r1: Vec<i8> = (0..(rng % 5) as usize)
                .map(|k| [1, -1, 2, -2][(rng.wrapping_add(k as u64) % 4) as usize])
                .collect();
            let key = (r0.clone(), r1.clone());
            let fresh = seen.insert(key);
            let got = store.insert_new(&r0, &r1, None, 0);
            assert_eq!(got.is_some(), fresh, "membership disagrees at {i}");
            if fresh {
                distinct += 1;
            }
        }
        assert_eq!(store.len(), distinct);
        assert_eq!(store.len() as usize, seen.len());
    }
}
