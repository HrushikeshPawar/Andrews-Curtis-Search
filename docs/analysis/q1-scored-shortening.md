# Q1 scored shortening

Status: full 1,591-certificate induced-closure benchmark complete; public-best comparison remains pending an external leaderboard snapshot.

## Objective

Q1 treats the accepted certificate inventory as a shared exact-state transport
network. The first implementation slice builds one packed global graph, adds
induced official primitive edges, computes reverse shortest distances to
`[[1],[2]]`, and reconstructs replay-verified candidates for every ingested
root.

## Implemented

- `TrajectoryGraph` inserts the exact target and deduplicates reduced ordered
  presentations using the existing packed state arena.
- Certificate ingestion records challenge roots, original trajectory edges,
  raw trajectory-state count, unique-state count, and target ID.
- Induced closure applies all 14 official moves to every known state and keeps
  only destinations already in the global store.
- Reverse BFS records one next-state/move pair per reachable state; it does not
  duplicate suffix byte arrays.
- Every reconstructed candidate is replay-gated through `ac-core`.
- `StartCatalog` joins scored result JSONL or submission text to exact starts
  from a SAIR manifest.
- Priority ranking emits deterministic P0/P1/P2/P3/P4/Unknown buckets.
- Quality GSSUB continues after bank hits and retains only shorter replay-valid
  incumbents. Existing first-hit GSSUB behavior is unchanged.
- `q1_graph` writes atomically replaced `graph_stats.json` and
  `candidates.jsonl`; it never uploads submissions.

## Local verification

Passed on this branch:

```text
cargo fmt --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace  # 67 passed, 2 ignored
```

The focused graph tests cover exact ordering, deduplication, induced closure,
reverse reconstruction, and replay. The quality test covers improvement of an
incumbent after a bank hit.

## Real-data run

The private SAIR checkout supplied the official scored-start manifest:

```text
/mnt/SharedDrive/ActiveProjects/Andrews-Curtis Conjecture Challenge/sair-acc/competition/tools/verifier/data/manifest.json
```

The complete accepted inventory required the three existing 500/500/356
submission parts plus the 235-case S4.3 GSSUB tail file:

```bash
cargo run --release -p ac-search --example q1_graph -- \
  "$SAIR_SCORED_MANIFEST" \
  results/q1_scored_shortening \
  results/acc_submission_ac_part01_of_03.txt \
  results/acc_submission_ac_part02_of_03.txt \
  results/acc_submission_ac_part03_of_03.txt \
  results/s4_3_gssubtail_new_vs_upload.txt
```

Measured graph run:

| metric | value |
| --- | ---: |
| source certificates | 1,591 |
| trajectory states before deduplication | 2,127,024 |
| unique exact states | 614,716 |
| deduplication ratio | 71.1% |
| original primitive edges | 2,125,433 |
| primitive moves tested in closure | 8,606,024 |
| closure edges landing in known states | 1,333,290 |
| reverse-reachable states | 614,716 |
| roots shortened | 1,433 |
| total moves removed | 1,436,851 |
| old median / p90 | 769 / 3,549 |
| new median / p90 | 196 / 1,053 |

All 1,591 reconstructed candidates passed `ac-core` replay. The 1,433
shortened candidates were split into verifier-safe batches of 500, 500, and
433. The official SAIR verifier accepted all three batches: 1,433/1,433
results passed with zero failures. These receipts and candidate files are
under `results/q1_scored_shortening/`.

This is an internal shortening result, not yet a leaderboard result. The
supplied SAIR checkout exposes the official verifier and challenge manifest,
but no current per-challenge public-best export. Therefore this report makes
no claim that any P0 tie crossed the public frontier or that score increased.

## Next compute decision

Next, obtain a timestamped public-best snapshot and join it to the machine-
readable candidate inventory. Then rank P0/P1 cases and spend stronger compute
only where a score change is possible: radius-2 bridges, quality GSSUB, beam
shortening, and exact bounded search for short targets. The induced graph is
already valuable as a shared suffix atlas; it should be retained and refreshed
after every verified shortening batch.

## Targeted radius-2 result

An all-state radius-2 scan was rejected as a production strategy: a 500-case
probe reached approximately 18 GB resident memory before the execution window
terminated. The implementation was therefore restricted to scoring roots and
bounded by old certificate length.

The first short-case run scanned the 112 roots with old length at most 50:

| metric | value |
| --- | ---: |
| roots scanned | 112 |
| two-move paths tested | 21,952 |
| destinations already in atlas | 1,809 |
| internal root improvements | 2 |
| verifier-clean new improvements | 2 |

The new verifier-clean candidates are `ac-04359` (21 to 20) and `ac-09594`
(19 to 18). Both passed the official verifier and are saved under
`results/q1_scored_shortening_radius2_short50/`. Public-best status is still
unknown, so these are prepared improvements rather than claimed score gains.

A follow-up through old length 100 scanned 298 reachable roots and tested
58,408 two-move paths. It produced one additional improvement, `ac-00766`
(69 to 67); together with the two short-case results, all three candidates
passed the official verifier. The run is recorded in
`results/q1_scored_shortening_radius2_short100/`. Because the yield is sparse
and public-best lengths are still unavailable, the next compute spend should
move to public-frontier acquisition and exact P0/P1 targeting rather than
blindly widening radius-2.

## Q1.5 quality GSSUB result

Quality mode was wired to continue after bank hits and compare every verified
completion against the current incumbent. Its bank was built from the global
shortened-certificate corpus (217,446 exact states), while the frozen
first-hit GSSUB path was left unchanged. The runner appends and flushes one
JSONL row per case and skips completed IDs on restart.

The ≤50 pilot completed 130 cases at 100,000 generated-witness work per case:
6 candidates shortened, removing 35 moves total, and all 6 passed the
official verifier. The strongest reduction was `ac-05703`, 50 to 34.

The ≤100 run completed a 73-case prefix of 298 eligible cases before being
stopped for handoff. It found 7 improvements removing 78 moves total; all 7
passed the official verifier. The strongest reductions were `ac-01350`, 85 to
67, `ac-00946`, 71 to 54, and `ac-00487`, 89 to 73. Both runs are resumable
from their existing `candidates.jsonl` files.

These are internal quality improvements, not leaderboard claims until joined
with a public-best snapshot. The ≤100 quality run can be resumed when compute
is available; after the beam slice, the next original-plan arm is exact
bounded search for short P0/P1 targets.

## Q1.6 targeted beam result

Beam-64 was run against the same 130-case ≤50 incumbent slice with a one-
million-work budget per case. It produced 19 strict improvements removing 93
moves in aggregate. Every strict improvement replayed through `ac-core` and
passed the official SAIR verifier. The strongest reductions were `ac-05091`
(42 to 27), `ac-06645` (32 to 18), and `ac-05668` (37 to 25).

Beam output is intentionally compared against the incumbent before promotion:
many beam solutions were longer, so they were retained only as negative
portfolio observations and were not included in the verifier batch. Results
are under `results/q1_beam_short50/` and are resumable by challenge ID.
