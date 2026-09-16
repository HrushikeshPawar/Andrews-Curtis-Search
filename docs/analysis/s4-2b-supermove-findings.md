# S4.2b supermove findings (first formulation)

Two formulations of witnessed substitution supermoves were implemented and
measured against the frozen 50-case residue pilot (same ids, same medium
budget, equal primitive-work accounting: one macro costs its witness length).

## What was implemented

`crates/ac-search/src/supermove.rs` generates composite proposals from real
primitive witnesses:

- rotate the target relator and/or the source relator by a short conjugator
  word (suffix or inverse prefix of that relator, so the conjugation is a
  length-neutral cyclic rotation), then
- multiply the (rotated) source into the target, optionally inverted.

Each proposal carries the exact successor, the full primitive witness, and the
signed net length change. Proposals are ranked by letters cancelled per witness
move; a small quota of length-non-decreasing proposals is kept as setup moves.
`propose_all` exposes the raw family for measurement.

## Measurements

**Residue frontier is uncancellable.** On real residue starts the rotation
family finds match lengths of 1-2 letters: net length change is `|appended| − 2m`,
so nothing cancels (best candidate on ac-00001 adds 5 letters; 36 candidates at
`max_conjugator = 1`, zero with positive cancellation; even at
`max_conjugator = 2` with 100 candidates the best is −1). Sampled across the
frozen runs, macros cancelled ~0.03 letters per expansion.

**Known corridors are cancellable.** Sampling 2,000 banked atlas states (every
269th of the 539,769): at `max_conjugator = 1`, 24.1% have at least one
cancelling supermove (0.58 cancelled letters/state, best cancellations up to
+14); at `max_conjugator = 2`, 40.9% (1.25 letters/state). So
cancellation-rich states exist on real solution paths while the residue
frontier has none: the asymmetry is not a tuning artifact.

**Pilot result (50 cases, medium budget).** All arms solve the same single
bank hit and nothing else; the macro arm explores 30% fewer distinct states per
primitive unit:

| arm | solved | macros | witness moves | macro share of budget | cancelled letters (per macro) |
|---|---|---|---|---|---|
| atlas-greedy (frozen) | 1/50 | — | — | 0 | — |
| supermove v1 (source rotations only) | 1/50 | 5.9M | 11.5M | 23.5% | 270,808 (0.05) |
| supermove v2 (rotations + un-rotation) | 1/50 | 4.5M | 20.2M | 41.2% | 111,762 (0.025) |
| supermove v3 (rotations, no un-rotation) | 1/50 | 5.6M | 15.3M | 31.3% | 719,439 (0.128) |

v3 (the corrected family) cancels ~7x more per macro than v1, but still finds
no new coverage: single-round cancellation is too rare per primitive charged.

## Why single-round macros are inert (structural)

Conjugation by a suffix or inverse prefix of a relator is a *length-neutral*
rotation. Therefore a single-round macro is a 2-3 move witness whose
intermediate states sit at the same total length as the source state. A
length-greedy search expands all states of a given length before any longer
one, so it already visits the rotated intermediate state at the same tier;
the macro only shortcuts ordering, it does not add reachability. The pilot
confirms this: the macro arm explores fewer distinct states and solves the
same cases.

## Implication

The value AC-SolverX attributes to cancellation-seeking supermoves must come
from composite steps whose intermediate states are *longer* and therefore
deprioritized (possibly never expanded) by length-greedy: i.e. grow-then-cancel
chains packaged into a single witnessed macro with net-negative length. The
next experiment is a two-round generator: compose the best length-increasing
proposals with the cancelling proposals available at their successors, keep
compositions with net cancellation, charge the summed witness. The single-round
family tested here should not be promoted.

## Reproduction

```bash
export SAIR_ACC_ROOT=/path/to/sair-acc
cargo build --release -p ac-search --examples
./target/release/examples/supermove_stats ac-00001 ac-00018   # per-state detail
./target/release/examples/supermove_stats bank:2000           # corridor sample
./target/release/examples/s3_sweep 0 1 subst medium scored tag idfile.json
```
