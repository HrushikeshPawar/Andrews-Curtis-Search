# Q1 scored shortening

Status: implementation slice complete; full scored benchmark pending the external scored-start manifest.

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

The scored-start manifest must be supplied from the private SAIR checkout. The
runner accepts the existing submission files and/or JSONL sweep outputs:

```bash
cargo run --release -p ac-search --example q1_graph -- \
  "$SAIR_SCORED_MANIFEST" \
  results/q1_scored_shortening \
  results/acc_submission_ac_part01_of_03.txt \
  results/acc_submission_ac_part02_of_03.txt \
  results/acc_submission_ac_part03_of_03.txt
```

The current workspace does not contain that scored-start manifest, and
`SAIR_ACC_ROOT` is unset here. Therefore no 1,591-case graph-size, memory, or
leaderboard-shortening claim is made yet. The official verifier remains an
external gate and must be run before any generated candidate is treated as
submission-ready.

## Next compute decision

Run the global graph first and record state/edge scale. If induced closure
shortens P0/P1 cases, add bounded radius-2 bridges and feed verified shorter
trajectories back into the graph. If it produces no frontier movement, spend
compute on exact bounded search for the P0/P1 list rather than broad GSSUB
coverage.
