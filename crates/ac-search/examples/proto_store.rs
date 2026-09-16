//! PROTOTYPE — THROWAWAY. Branch: prototype/storage-alternatives. Do NOT merge.
//!
//! Question: which single-copy store design replaces the dual-owned
//! `Vec<Presentation>` + `HashMap<Presentation, u32>` without changing
//! exact-state equality or BFS traversal?
//!
//! One generic BFS loop over a `Store` trait; five implementations:
//!   dual       = current design (baseline): Vec + HashMap, 2 copies, 4 allocs
//!   packed     = (B) byte-packed words, still dual-stored, 2 allocs
//!   bump_ptr   = (C1) bump arena + pointer-keyed HashSet (tiny unsafe)
//!   bump_hb    = (C2) bump arena + hashbrown raw-table lookup (fully safe)
//!   flat       = (C3) hand-rolled linear-probing table over bump arena
//!
//! Run: cargo run --release -p ac-search --example proto_store -- <variant>
//!      <ms1y|fixed> [max_work]
//! Prints one machine-readable RESULT line. Compare across variants with
//! /tmp/opencode/sweep/proto_matrix.py (peak RSS via /usr/bin/time -v).

use ac_core::{
    DEFAULT_N_GEN, Move, Presentation, is_ac_target, move_from_id, reduce, replay_certificate,
};
use std::collections::{HashMap, HashSet};
use std::hash::{BuildHasher, Hash, Hasher};
use std::path::PathBuf;
use std::time::Instant;

// ---------------------------------------------------------------------------
// Packing: one byte per letter, b = (letter + 2) as u8. Injective on valid
// letters, so packed-byte equality <=> Presentation equality.
// Record layout: [len0:u32 LE][len1:u32 LE][r0 bytes][r1 bytes].
// ---------------------------------------------------------------------------

fn pack_into(buf: &mut Vec<u8>, r0: &[i8], r1: &[i8]) {
    buf.clear();
    buf.extend_from_slice(&(r0.len() as u32).to_le_bytes());
    buf.extend_from_slice(&(r1.len() as u32).to_le_bytes());
    buf.extend(r0.iter().map(|&l| (l + 2) as u8));
    buf.extend(r1.iter().map(|&l| (l + 2) as u8));
}

fn decode_into(d0: &mut Vec<i8>, d1: &mut Vec<i8>, rec: &[u8]) {
    let n0 = u32::from_le_bytes(rec[0..4].try_into().unwrap()) as usize;
    let n1 = u32::from_le_bytes(rec[4..8].try_into().unwrap()) as usize;
    d0.clear();
    d1.clear();
    d0.extend(rec[8..8 + n0].iter().map(|&b| b as i8 - 2));
    d1.extend(rec[8 + n0..8 + n0 + n1].iter().map(|&b| b as i8 - 2));
}

// ---------------------------------------------------------------------------
// Store trait: single shared BFS loop below is generic over this.
// ---------------------------------------------------------------------------

trait Store {
    fn len(&self) -> u32;
    /// Insert; None if already present (duplicate), Some(id) on discovery.
    fn insert_new(&mut self, r0: Vec<i8>, r1: Vec<i8>) -> Option<u32>;
    fn with_state<R>(&mut self, id: u32, f: impl FnOnce(&[i8], &[i8]) -> R) -> R;
}

// --- dual: current production design (baseline) -----------------------------

struct Dual {
    states: Vec<(Vec<i8>, Vec<i8>)>,
    map: HashMap<(Vec<i8>, Vec<i8>), u32>,
}

impl Store for Dual {
    fn len(&self) -> u32 {
        self.states.len() as u32
    }
    fn insert_new(&mut self, r0: Vec<i8>, r1: Vec<i8>) -> Option<u32> {
        use std::collections::hash_map::Entry;
        match self.map.entry((r0, r1)) {
            Entry::Occupied(_) => None,
            Entry::Vacant(v) => {
                let id = self.states.len() as u32;
                self.states.push(v.key().clone());
                v.insert(id);
                Some(id)
            }
        }
    }
    fn with_state<R>(&mut self, id: u32, f: impl FnOnce(&[i8], &[i8]) -> R) -> R {
        let (r0, r1) = &self.states[id as usize];
        f(r0, r1)
    }
}

// --- packed: byte-packed words, still dual-stored (alternative B) -----------

struct Packed {
    states: Vec<(Box<[u8]>, Box<[u8]>)>,
    map: HashMap<(Box<[u8]>, Box<[u8]>), u32>,
    decode: (Vec<i8>, Vec<i8>),
}

fn pack_word(w: &[i8]) -> Box<[u8]> {
    w.iter().map(|&l| (l + 2) as u8).collect()
}

impl Store for Packed {
    fn len(&self) -> u32 {
        self.states.len() as u32
    }
    fn insert_new(&mut self, r0: Vec<i8>, r1: Vec<i8>) -> Option<u32> {
        // NOTE: packs twice on the miss path (probe key + stored key); a
        // production version would pack once. Prototype measures structure.
        let probe = (pack_word(&r0), pack_word(&r1));
        if self.map.contains_key(&probe) {
            return None;
        }
        let id = self.states.len() as u32;
        self.states.push(probe.clone());
        self.map.insert(probe, id);
        Some(id)
    }
    fn with_state<R>(&mut self, id: u32, f: impl FnOnce(&[i8], &[i8]) -> R) -> R {
        let (p0, p1) = &self.states[id as usize];
        self.decode
            .0
            .extend(p0.iter().map(|&b| b as i8 - 2));
        self.decode
            .1
            .extend(p1.iter().map(|&b| b as i8 - 2));
        let r = f(&self.decode.0, &self.decode.1);
        self.decode.0.clear();
        self.decode.1.clear();
        r
    }
}

// --- bump arena shared by C1/C2/C3 ------------------------------------------

const CHUNK: usize = 64 << 20;

struct Bump {
    chunks: Vec<Box<[u8]>>,
    used: usize, // bytes used in last chunk
    count: u32,
    scratch: Vec<u8>, // pack buffer for probes
    decode: (Vec<i8>, Vec<i8>),
}

impl Bump {
    fn record(&self, handle: u64) -> &[u8] {
        let chunk = (handle >> 32) as usize;
        let off = (handle & 0xffff_ffff) as usize;
        let n0 = u32::from_le_bytes(self.chunks[chunk][off..off + 4].try_into().unwrap()) as usize;
        let n1 =
            u32::from_le_bytes(self.chunks[chunk][off + 4..off + 8].try_into().unwrap()) as usize;
        &self.chunks[chunk][off..off + 8 + n0 + n1]
    }
    fn append(&mut self, bytes: &[u8]) -> u64 {
        assert!(bytes.len() <= CHUNK, "record exceeds chunk size");
        if self.chunks.is_empty() || self.used + bytes.len() > self.chunks.last().unwrap().len() {
            self.chunks.push(vec![0u8; CHUNK].into_boxed_slice());
            self.used = 0;
        }
        let idx = self.chunks.len() - 1;
        let off = self.used;
        self.chunks[idx][off..off + bytes.len()].copy_from_slice(bytes);
        self.used += bytes.len();
        ((idx as u64) << 32) | off as u64
    }
}

// --- bump_ptr: C1, pointer-keyed set (tiny unsafe) ---------------------------
// Invariants: chunks are fixed-size boxes (never move); records are
// append-only and never mutated; every RawKey in the set points at a live
// committed record. Set drop never dereferences keys (Copy, no Drop).

#[derive(Clone, Copy)]
struct RawKey {
    ptr: *const u8,
    len: u32,
}

impl RawKey {
    fn bytes(&self) -> &[u8] {
        // SAFETY: by the invariants above.
        unsafe { std::slice::from_raw_parts(self.ptr, self.len as usize) }
    }
}

impl PartialEq for RawKey {
    fn eq(&self, other: &Self) -> bool {
        self.bytes() == other.bytes()
    }
}
impl Eq for RawKey {}
impl Hash for RawKey {
    fn hash<H: Hasher>(&self, h: &mut H) {
        self.bytes().hash(h);
    }
}

struct BumpPtr {
    bump: Bump,
    ids: Vec<u64>, // discovery id -> handle
    set: HashSet<RawKey>,
}

impl Store for BumpPtr {
    fn len(&self) -> u32 {
        self.bump.count
    }
    fn insert_new(&mut self, r0: Vec<i8>, r1: Vec<i8>) -> Option<u32> {
        pack_into(&mut self.bump.scratch, &r0, &r1);
        let probe = RawKey {
            ptr: self.bump.scratch.as_ptr(),
            len: self.bump.scratch.len() as u32,
        };
        if self.set.contains(&probe) {
            return None;
        }
        let bytes = std::mem::take(&mut self.bump.scratch);
        let handle = self.bump.append(&bytes);
        self.bump.scratch = bytes;
        self.bump.scratch.clear();
        let stored = RawKey {
            ptr: self.bump.record(handle).as_ptr(),
            len: self.bump.record(handle).len() as u32,
        };
        self.set.insert(stored);
        let id = self.bump.count;
        self.bump.count += 1;
        self.ids.push(handle);
        Some(id)
    }
    fn with_state<R>(&mut self, id: u32, f: impl FnOnce(&[i8], &[i8]) -> R) -> R {
        let rec = self.bump.record(self.ids[id as usize]).to_vec();
        decode_into(&mut self.bump.decode.0, &mut self.bump.decode.1, &rec);
        f(&self.bump.decode.0, &self.bump.decode.1)
    }
}

// --- bump_hb: C2, hashbrown raw table over the same bump arena (safe) --------

struct BumpHb {
    bump: Bump,
    ids: Vec<u64>,
    table: hashbrown::HashTable<u32>,
    build: std::collections::hash_map::RandomState,
}

impl Store for BumpHb {
    fn len(&self) -> u32 {
        self.bump.count
    }
    fn insert_new(&mut self, r0: Vec<i8>, r1: Vec<i8>) -> Option<u32> {
        pack_into(&mut self.bump.scratch, &r0, &r1);
        let hash = {
            let mut h = self.build.build_hasher();
            self.bump.scratch.hash(&mut h);
            h.finish()
        };
        {
            let bump = &self.bump;
            let ids = &self.ids;
            let scratch: &[u8] = &bump.scratch;
            if self
                .table
                .find(hash, |&id| bump.record(ids[id as usize]) == scratch)
                .is_some()
            {
                return None;
            }
        }
        let bytes = std::mem::take(&mut self.bump.scratch);
        let handle = self.bump.append(&bytes);
        self.bump.scratch = bytes;
        self.bump.scratch.clear();
        let id = self.bump.count;
        self.bump.count += 1;
        self.ids.push(handle);
        let rec = self.bump.record(handle).to_vec();
        let hash2 = {
            let mut h = self.build.build_hasher();
            rec.hash(&mut h);
            h.finish()
        };
        // Table bookkeeping re-hash of the stored record (prototype-grade).
        let bump = &self.bump;
        let ids = &self.ids;
        let build = self.build.clone();
        self.table.insert_unique(hash2, id, |&sid| {
            let mut h = build.build_hasher();
            bump.record(ids[sid as usize]).hash(&mut h);
            h.finish()
        });
        Some(id)
    }
    fn with_state<R>(&mut self, id: u32, f: impl FnOnce(&[i8], &[i8]) -> R) -> R {
        let rec = self.bump.record(self.ids[id as usize]).to_vec();
        decode_into(&mut self.bump.decode.0, &mut self.bump.decode.1, &rec);
        f(&self.bump.decode.0, &self.bump.decode.1)
    }
}

// --- flat: C3, hand-rolled linear probing over bump arena (safe, std) --------

fn probe_slot(
    slots: &[u32],
    mask: usize,
    bump: &Bump,
    ids: &[u64],
    bytes: &[u8],
    hash: u64,
) -> (usize, bool) {
    let mut i = (hash as usize) & mask;
    loop {
        let id = slots[i];
        if id == u32::MAX {
            return (i, false);
        }
        if bump.record(ids[id as usize]) == bytes {
            return (i, true);
        }
        i = (i + 1) & mask;
    }
}

struct Flat {
    bump: Bump,
    ids: Vec<u64>,
    slots: Vec<u32>, // u32::MAX = empty
    mask: usize,
    filled: usize,
}

impl Flat {
    fn hash_bytes(bytes: &[u8]) -> u64 {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        bytes.hash(&mut h);
        h.finish()
    }
    fn grow(&mut self) {
        // Records stay put in the bump arena; only the slot table is rebuilt.
        let new_cap = (self.slots.len() * 2).max(16);
        let mut slots = vec![u32::MAX; new_cap];
        let mask = new_cap - 1;
        for (id, &handle) in self.ids.iter().enumerate() {
            let rec = self.bump.record(handle);
            let h = Self::hash_bytes(rec);
            let (slot, found) = probe_slot(&slots, mask, &self.bump, &self.ids, rec, h);
            debug_assert!(!found);
            slots[slot] = id as u32;
        }
        self.slots = slots;
        self.mask = mask;
    }
}

impl Store for Flat {
    fn len(&self) -> u32 {
        self.bump.count
    }
    fn insert_new(&mut self, r0: Vec<i8>, r1: Vec<i8>) -> Option<u32> {
        pack_into(&mut self.bump.scratch, &r0, &r1);
        let hash = Self::hash_bytes(&self.bump.scratch);
        let (mut slot, found) = probe_slot(
            &self.slots,
            self.mask,
            &self.bump,
            &self.ids,
            &self.bump.scratch,
            hash,
        );
        if found {
            return None;
        }
        if self.filled * 8 >= self.slots.len() * 7 {
            self.grow();
            let (s2, f2) = probe_slot(
                &self.slots,
                self.mask,
                &self.bump,
                &self.ids,
                &self.bump.scratch,
                hash,
            );
            debug_assert!(!f2);
            slot = s2;
        }
        let bytes = std::mem::take(&mut self.bump.scratch);
        let handle = self.bump.append(&bytes);
        self.bump.scratch = bytes;
        self.bump.scratch.clear();
        let id = self.bump.count;
        self.bump.count += 1;
        self.ids.push(handle);
        self.slots[slot] = id;
        self.filled += 1;
        Some(id)
    }
    fn with_state<R>(&mut self, id: u32, f: impl FnOnce(&[i8], &[i8]) -> R) -> R {
        let rec = self.bump.record(self.ids[id as usize]).to_vec();
        decode_into(&mut self.bump.decode.0, &mut self.bump.decode.1, &rec);
        f(&self.bump.decode.0, &self.bump.decode.1)
    }
}

// ---------------------------------------------------------------------------
// Shared BFS loop (identical traversal for every store).
// ---------------------------------------------------------------------------

fn run<S: Store>(mut store: S, root: Presentation, max_work: u64) {
    let t0 = Instant::now();
    let table: [Move; 14] =
        core::array::from_fn(|i| move_from_id(i as u8).expect("ids 0..=13 are pinned"));
    let mut parents: Vec<Option<(u32, u8)>> = vec![None];
    let mut depths: Vec<u32> = vec![0];
    let mut queue: std::collections::VecDeque<u32> = Default::default();
    store
        .insert_new(root.relators[0].clone(), root.relators[1].clone())
        .expect("root is new");
    queue.push_back(0);
    let (mut expanded, mut generated, mut dups, mut max_front) = (0u64, 0u64, 0u64, 1u64);
    let mut solved: Option<Vec<u8>> = None;
    if is_ac_target(&root) {
        solved = Some(Vec::new());
    }
    while let Some(si) = queue.pop_front() {
        if solved.is_some() || generated >= max_work {
            break;
        }
        let depth = depths[si as usize];
        let mut done_all = true;
        for id in 0..14u8 {
            if generated >= max_work {
                done_all = false;
                break;
            }
            let nxt: Presentation = store.with_state(si, |r0, r1| {
                let cur = Presentation {
                    relators: [r0.to_vec(), r1.to_vec()],
                };
                ac_core::apply_move(&cur, &table[id as usize], DEFAULT_N_GEN).unwrap()
            });
            generated += 1;
            match store.insert_new(nxt.relators[0].clone(), nxt.relators[1].clone()) {
                None => dups += 1,
                Some(child) => {
                    parents.push(Some((si, id)));
                    depths.push(depth + 1);
                    if is_ac_target(&Presentation {
                        relators: [nxt.relators[0].clone(), nxt.relators[1].clone()],
                    }) {
                        let mut cert = Vec::new();
                        let mut cur = child;
                        while let Some((p, m)) = parents[cur as usize] {
                            cert.push(m);
                            cur = p;
                        }
                        cert.reverse();
                        assert!(replay_certificate(&root, &cert).is_ok());
                        solved = Some(cert);
                        break;
                    }
                    queue.push_back(child);
                    max_front = max_front.max(queue.len() as u64);
                }
            }
        }
        if done_all {
            expanded += 1;
        }
        if solved.is_some() {
            break;
        }
    }
    let status = if solved.is_some() {
        "solved"
    } else if generated >= max_work {
        "limit"
    } else {
        "exhausted"
    };
    println!(
        "RESULT status={} cert={:?} expanded={} generated={} unique={} dups={} frontier={} ms={}",
        status,
        solved.unwrap_or_default(),
        expanded,
        generated,
        store.len(),
        dups,
        max_front,
        t0.elapsed().as_millis()
    );
}

fn reduced(r0: Vec<i8>, r1: Vec<i8>) -> Presentation {
    Presentation {
        relators: [
            reduce(&r0, DEFAULT_N_GEN).unwrap(),
            reduce(&r1, DEFAULT_N_GEN).unwrap(),
        ],
    }
}

fn main() {
    let mut args = std::env::args().skip(1);
    let variant = args.next().unwrap_or_else(|| "dual".into());
    let workload = args.next().unwrap_or_else(|| "ms1y".into());
    let max_work: u64 = args
        .next()
        .and_then(|s| s.parse().ok())
        .unwrap_or(u64::MAX);
    let root = match workload.as_str() {
        "ms1y" => reduced(vec![-1, 2, 1, -2, -2], vec![-1, 2]),
        "fixed" => {
            let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../vendor/sair");
            let item = ac_core::data::load_training(dir.join("examples/training_424.json"))
                .unwrap()
                .into_iter()
                .find(|x| x.training_id == "ms-train-0004")
                .unwrap();
            reduced(
                item.initial_relators[0].clone(),
                item.initial_relators[1].clone(),
            )
        }
        other => panic!("unknown workload {other}"),
    };
    eprintln!("VARIANT={variant} WORKLOAD={workload}");
    match variant.as_str() {
        "dual" => run(
            Dual {
                states: Vec::new(),
                map: HashMap::new(),
            },
            root,
            max_work,
        ),
        "packed" => run(
            Packed {
                states: Vec::new(),
                map: HashMap::new(),
                decode: (Vec::new(), Vec::new()),
            },
            root,
            max_work,
        ),
        "bump_ptr" => run(
            BumpPtr {
                bump: Bump {
                    chunks: Vec::new(),
                    used: 0,
                    count: 0,
                    scratch: Vec::new(),
                    decode: (Vec::new(), Vec::new()),
                },
                ids: Vec::new(),
                set: HashSet::new(),
            },
            root,
            max_work,
        ),
        "bump_hb" => run(
            BumpHb {
                bump: Bump {
                    chunks: Vec::new(),
                    used: 0,
                    count: 0,
                    scratch: Vec::new(),
                    decode: (Vec::new(), Vec::new()),
                },
                ids: Vec::new(),
                table: hashbrown::HashTable::new(),
                build: std::collections::hash_map::RandomState::new(),
            },
            root,
            max_work,
        ),
        "flat" => run(
            Flat {
                bump: Bump {
                    chunks: Vec::new(),
                    used: 0,
                    count: 0,
                    scratch: Vec::new(),
                    decode: (Vec::new(), Vec::new()),
                },
                ids: Vec::new(),
                slots: vec![u32::MAX; 1024],
                mask: 1023,
                filled: 0,
            },
            root,
            max_work,
        ),
        other => panic!("unknown variant {other}"),
    }
}
