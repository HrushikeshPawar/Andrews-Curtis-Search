# Q1 Scored Shortening Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Turn the accepted certificate corpus into a shared exact-state graph that produces replay-verified shorter certificates and prioritizes compute by leaderboard payoff.

**Architecture:** `ac-core` remains the only semantic/replay authority. `ac-search` gains a packed `TrajectoryGraph` with global state IDs, original and induced primitive edges, reverse shortest-path metadata, and candidate reconstruction. `ac-cli` gains a thin corpus/graph runner and scoring artifact writer; quality GSSUB remains a separate mode and the frozen first-hit coverage path is unchanged.

**Tech Stack:** Rust workspace, `ac-core` exact moves/replay, existing packed `StateStore`, serde JSON artifacts, append-safe JSONL result records, GitButler.

**Spec:** Approved Q1 scored-shortening design from the preceding conversation; project boundary in `AGENTS.md`.

## Global Constraints

- Preserve official `ac-r2-v1` semantics and exact target `[[1],[2]]`.
- Do not canonicalize relator ordering, rotations, inversions, or Booth-equivalent states.
- Every saved candidate must replay through `ac-core`; official verification remains external through `SAIR_ACC_ROOT`.
- Preserve frozen coverage solver behavior and retain GSSUB as a coverage engine.
- Do not upload SAIR submissions automatically.
- Keep long jobs deterministic where practical, resumable, and append-safe.
- Use GitButler for commits and pushes; do not include unrelated existing uncommitted artifacts.

---

### Task 1: Expose reusable packed exact-state indexing

**Files:**
- Modify: `crates/ac-search/src/store.rs`
- Modify: `crates/ac-search/src/lib.rs`
- Test: `crates/ac-search/src/store.rs`

**Interfaces:**
- Produces a public-in-crate `PackedStateIndex` wrapper with `insert`, `find`, `copy_state`, and `len` operations.
- Keeps `StateId` stable and preserves the existing BFS store behavior.

- [ ] Write tests proving exact ordered presentations receive distinct IDs and duplicate insertions reuse IDs.
- [ ] Run `cargo test -p ac-search store_tests` and confirm the new tests fail because the wrapper is absent.
- [ ] Implement the smallest wrapper around the existing packed arena and membership table; do not duplicate payloads.
- [ ] Run the focused tests, then `cargo test -p ac-search`.
- [ ] Commit as `refactor(search): expose packed exact-state index`.

### Task 2: Implement global trajectory ingestion

**Files:**
- Create: `crates/ac-search/src/trajectory_graph.rs`
- Modify: `crates/ac-search/src/lib.rs`
- Test: `crates/ac-search/src/trajectory_graph.rs`

**Interfaces:**
- `TrajectoryGraph::new()` creates an empty graph and inserts the exact target.
- `TrajectoryGraph::ingest_certificate(challenge_id, start, certificate)` replays and inserts every state, records original edges, and returns the root `StateId`.
- `TrajectoryGraph::stats()` returns source count, raw trajectory states, unique states, original edge count, root count, and target ID.
- `TrajectoryGraph::state(id)` copies an exact state for tests and reconstruction.

- [ ] Write a fixture test that ingests two certificates sharing a suffix and asserts deduplication, root IDs, target presence, and exact state ordering.
- [ ] Run the focused test and observe failure because the graph module is absent.
- [ ] Implement ingestion by reducing the start exactly as `replay_certificate` does, applying official moves in order, and inserting each state into the packed index.
- [ ] Reject invalid move IDs and replay failures without leaving partial certificate metadata.
- [ ] Run focused tests and workspace tests.
- [ ] Commit as `feat(search): ingest global exact trajectories`.

### Task 3: Add induced primitive closure and reverse shortest paths

**Files:**
- Modify: `crates/ac-search/src/trajectory_graph.rs`
- Test: `crates/ac-search/src/trajectory_graph.rs`

**Interfaces:**
- `induce_primitive_edges()` applies all 14 official moves to every known state and records known destinations only.
- `reverse_shortest_paths()` computes `best_known_distance` and `next_move` from the target over induced edges.
- `shortest_certificate(root_id)` reconstructs the graph candidate and returns `None` when the root cannot reach the target.
- `closure_stats()` reports tested moves, known destinations, reachable states, and elapsed time.

- [ ] Write a test with a one-move fixture asserting the reverse distance and reconstructed certificate are one move and replay to target.
- [ ] Run the focused test and verify it fails before closure/path code exists.
- [ ] Implement deterministic state iteration, exact move application/reduction, adjacency storage, reverse traversal, and path reconstruction without changing `ac-core`.
- [ ] Assert reconstructed candidates with `replay_certificate` before returning them.
- [ ] Run focused tests, full workspace tests, and a release-mode fixture benchmark.
- [ ] Commit as `feat(search): add induced closure shortest paths`.

### Task 4: Add corpus loaders and resumable graph artifacts

**Files:**
- Create: `crates/ac-search/src/corpus.rs`
- Modify: `crates/ac-search/src/lib.rs`
- Modify: `crates/ac-search/Cargo.toml` only if an existing dependency is insufficient
- Test: `crates/ac-search/src/corpus.rs`

**Interfaces:**
- `ScoredCertificate { challenge_id, start, certificate, old_length, source_path }` is the normalized corpus record.
- `load_scored_certificates(paths)` reads existing JSON/JSONL/certificate-sidecar formats and deduplicates by challenge ID, retaining the shortest replay-valid record.
- `GraphCheckpoint` serializes graph metadata, roots, and completed ingestion source paths for restart detection.
- `write_jsonl_atomic(path, records)` writes a temporary sibling and renames it into place.

- [ ] Add fixture tests for JSONL records, sidecar certificates, duplicate challenge IDs, invalid certificates, and atomic result output.
- [ ] Run focused tests and verify missing loader functions fail.
- [ ] Implement adapters for the repository’s existing S3/GSSUB result shapes without assuming one experiment filename.
- [ ] Ensure malformed records are reported with source and challenge ID, while valid records continue when configured for resumable batch mode.
- [ ] Run loader tests and a real-data dry run that counts accepted records without building the full graph.
- [ ] Commit as `feat(search): load scored certificate corpus`.

### Task 5: Add scoring-priority ranking and Q1 report artifacts

**Files:**
- Create: `crates/ac-search/src/shortening.rs`
- Modify: `crates/ac-search/src/lib.rs`
- Create: `crates/ac-cli/src/bin/q1_scored_shortening.rs` or an existing thin CLI subcommand if that matches current CLI structure
- Create: `docs/analysis/q1-scored-shortening.md`
- Test: `crates/ac-search/src/shortening.rs`

**Interfaces:**
- `PriorityRecord { challenge_id, our_length, public_best, gap, bucket }` computes P0–P4 from measured public-best data.
- `ShorteningRecord` stores old/new lengths, method, commit, budgets, public-best snapshot, and replay/official-verifier status.
- The runner supports `--input`, `--output-dir`, `--ids`, and `--resume`, never uploads, and emits candidate batches capped at 500 records.

- [ ] Write ranking tests for ties, gaps 1–5, 6–20, 21–100, and larger gaps.
- [ ] Run focused tests and observe missing ranking behavior.
- [ ] Implement ranking, graph run orchestration, candidate comparison, replay gating, JSONL provenance, and report summary aggregation.
- [ ] Run the runner on a small real scored subset, inspect artifacts, and verify no upload path exists.
- [ ] Commit as `feat(cli): add scored-shortening graph runner`.

### Task 6: Add quality-mode GSSUB and selective bridge hooks

**Files:**
- Modify: `crates/ac-search/src/substitution.rs`
- Modify: `crates/ac-search/src/lib.rs`
- Create or modify: `crates/ac-search/src/local_bridge.rs`
- Test: substitution and bridge modules

**Interfaces:**
- Existing first-bank-hit GSSUB entry points retain their current behavior.
- New `QualitySearchConfig` accepts an incumbent, time/work limits, and a shortest-suffix lookup callback.
- Quality search continues after bank hits, evaluates `prefix_cost + suffix_cost`, and emits only replay-verified improvements.
- `LocalBridgeSearch` supports bounded deduplicated depth-2 search globally and selective depth-3 search by priority list.

- [ ] Add regression tests proving default GSSUB stops as before and quality mode can replace a longer incumbent through a shorter bank suffix.
- [ ] Add bridge tests proving duplicate suppression and exact replay of a retained bridge.
- [ ] Run focused tests and observe failures before production changes.
- [ ] Implement quality mode with deterministic tie-breaking and bounded work; do not alter coverage-mode defaults.
- [ ] Run focused tests and compare quality-mode metrics on real P0/P1 records.
- [ ] Commit as `feat(search): add anytime quality shortening`.

### Task 7: Real 1,591-case benchmark, verification, and handoff

**Files:**
- Modify: `docs/analysis/q1-scored-shortening.md`
- Create: `results/q1_scored_shortening/` artifacts only for generated machine-readable outputs

- [ ] Run the full graph ingestion, induced closure, and reverse shortest-path pass over all available accepted scored certificates.
- [ ] Record graph size, dedup ratio, tested/induced edge counts, runtime, and memory observations.
- [ ] Run P0/P1 quality and bridge arms, then P2 if measured returns justify it.
- [ ] Replay every candidate through `ac-core` and run the official verifier for candidates when `SAIR_ACC_ROOT` is available.
- [ ] Produce old/new inventory, gap distributions, shortened IDs, method provenance, verifier results, and ≤500 candidate submission batches.
- [ ] Run `cargo fmt --check`, Clippy, workspace tests, and official verification gates.
- [ ] Update the report with negative results and compute recommendations.
- [ ] Commit artifacts/report separately as `data: record q1 scored-shortening results`.

