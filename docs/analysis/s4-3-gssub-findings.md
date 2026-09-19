# S4.3 witnessed substitution search — 500-case residue

## Implementation summary

New experimental solver `gssub` (`crates/ac-search/src/substitution.rs`):

- Action: exact state → cancelling substitution macro → exact state.
- Macro: rotate target and other, optional invert of other, left or right
  multiply, restore the other relator, cyclic-reduce the written relator.
- Every macro carries an official primitive witness. Booth canonicalization
  is excluded. Search state is the exact ordered presentation.
- Search is length-greedy over **macro edges only** (no primitive fanout).
- Bank probing matches S4.1; reverse ball is off.
- Frozen primitive greedy/beam and S4.2b `subst` are unchanged.

Witness design: `docs/analysis/s4-3-substitution-witness.md`.
Tests: `substitution::substitution_tests` (destination == witness replay).

Git commit: `29b690e31bd93c2a9542dac86256805defc5b8ef`.

## Commands

```bash
export SAIR_ACC_ROOT=/path/to/sair-acc
cargo build --release -p ac-search --examples
for i in 0 1 2 3; do
  ./target/release/examples/s3_sweep $i 4 gssub medium scored gssub500 \
    results/atlas_slice_500.json &
done
wait
./scripts/run-official-verifier.sh \
  --manifest "$SAIR_ACC_ROOT/competition/tools/verifier/data/manifest.json" \
  --submission results/s4_3_gssub500_all39.txt --pretty
```

Budget: medium, `max_work = 1_000_000` primitive-equivalent (witness length).
Policy: `GSSUB_TOP_K=16`. Slice: frozen `results/atlas_slice_500.json`.

## Results (500 residue cases)

| arm | solved | new vs atlas-19 | official verifier |
|---|---|---|---|
| atlas-greedy medium (frozen) | 19/500 | — | (prior) |
| S4.2b subst K=2 cancelling | 23/500 | +4 | (prior) |
| **gssub medium** | **39/500** | **+20** | **39/39 accepted** |

- All 19 atlas solves retained (no losses).
- All 23 S4.2b K=2 solves retained.
- **16 IDs new vs the union of atlas-19 and S4.2b subst.**
- All 39 are outside the S3.1 814 inventory (the slice definition).
- All 39 are bank hits: the macro walk reached a banked state; the emitted
  certificate is the reconstructed prefix plus the stored suffix.
- Internal `replay_ok` on all 39; official SAIR verifier accepted all 39.

New vs atlas+S4.2b (officially verified):

`ac-00017`, `ac-00023`, `ac-00048`, `ac-00136`, `ac-00154`, `ac-00243`,
`ac-00281`, `ac-00345`, `ac-00350`, `ac-00376`, `ac-00400`, `ac-00452`,
`ac-00460`, `ac-00486`, `ac-00514`, `ac-00536`.

Submissions: `results/s4_3_gssub500_new16.txt`, `results/s4_3_gssub500_all39.txt`.
Summary: `results/s4_3_gssub500_summary.json`. Shard records:
`results/s3_gssub500_scored_gssub_medium_shard_{0..3}.json`.

### Search accounting (500 cases)

| metric | total |
|---|---|
| macro transitions generated | 41,533,492 |
| primitive witness moves charged | 472,199,726 |
| letters cancelled (reported) | 13,965,019 |
| nodes expanded | 2,598,991 |
| wall (4-way parallel) | 4516 s |

Peak RSS was not collected.

## Comparison

The frozen question is new coverage, not shorter certs on already-solved
cases. gssub more than doubles atlas-greedy coverage on this slice
(19 → 39) and adds 16 cases S4.2b short-conjugator supermoves missed.
The mechanism is the ACSolverX-style edge: source restore plus wrap-around
cyclic reduction, enumerated over all cancelling alignments rather than
`max_conjugator ∈ {1,2}`.

Every solve is still a bank hit. The new action space is better at
**reaching the atlas**, not at closing to `[[1],[2]]` from scratch.

One S4.2b-noinc win (`ac-00557`) was not recovered. Macros-only can miss
a corridor that mixed primitive+macro found.

## Failures / limitations

- Residue starts still rarely net-decrease on the first expansion; most
  budget is spent walking length-increasing cancelling products until a
  banked state appears.
- Certificates are long (bank suffixes, 398–3801 primitives). No
  shortening pass.
- ACSolverX Booth/free cyclic canonicalization remains excluded, correctly.
- No GPU / no pretrained policy in this solver.

## Recommendation: SCALE

Coverage on the frozen 500-case slice is meaningful and verified
(+20 vs atlas, +16 vs best prior subst, 0 losses vs atlas, 39/39
official). Next step is a staged sweep of the remaining scored unsolved
set (9,262 IDs after this 39):

```bash
# remaining ids: results/s4_3_unsolved_after_gssub500.json
for i in 0 1 2 3 4 5 6 7; do
  ./target/release/examples/s3_sweep $i 8 gssub medium scored gssubrest \
    results/s4_3_unsolved_after_gssub500.json &
done
```

Do not go back to primitive beam widening. Do not convert the Part A
ACSolverX S-move paths until this sweep is in flight.
