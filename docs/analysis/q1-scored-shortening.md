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
