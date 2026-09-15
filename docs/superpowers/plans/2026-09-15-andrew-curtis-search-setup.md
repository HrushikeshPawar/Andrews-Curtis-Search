# Andrew-Curtis Search Setup Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Prepare a local Rust workspace with a trusted ordinary-AC kernel, official-data/verifier plumbing, and search interfaces, while leaving exact-state BFS unimplemented.

**Architecture:** `ac-core` owns all mathematical semantics and replay. `ac-search` owns limits, metrics, result/experiment records, and an explicit unimplemented solver seam. `ac-cli` is a thin binary for replay, verify, submission writing, data loading, and the future solve command. Official Python verifier code remains an external process selected by `SAIR_ACC_ROOT`.

**Tech Stack:** Stable Rust, Cargo workspace, serde/serde_json, sha2, clap; official SAIR Python verifier as a subprocess.

**Spec:** User request in the task conversation.

## Global Constraints

- Pin ordinary semantics to `ac-r2-v1` and the official SAIR move-spec hash.
- Preserve freely reduced ordered rank-2 presentations and exact target `[[1],[2]]`.
- Do not implement BFS or any other search algorithm.
- Keep official Python verification outside the Rust repository boundary.
- Keep the repository local: no CI, Docker, Python package, ML stack, or remote.

---

### Task 1: Scaffold workspace and provenance

- [ ] Create the root Cargo workspace and three crates with Cargo commands.
- [ ] Copy only shared fixtures and small official immutable artifacts needed by tests.
- [ ] Document scratch/upstream provenance and pinned hashes.

### Task 2: Port and test `ac-core`

- [ ] Port the scratch free-reduction and typed move kernel without semantic redesign.
- [ ] Add official move-spec validation, certificate replay, and typed/raw boundary APIs.
- [ ] Add fixture, official golden, and malformed-input tests.

### Task 3: Add loaders, verifier boundary, and records

- [ ] Add official training/challenge manifest loaders.
- [ ] Add external verifier invocation and submission serialization.
- [ ] Add versioned JSON experiment records.

### Task 4: Add search interfaces and CLI plumbing

- [ ] Define search limits, metrics, status, result, and parent/reconstruction primitives.
- [ ] Leave solver entrypoint explicitly unimplemented.
- [ ] Add replay/verify/write-submission/solve commands.

### Task 5: Verify and review boundary

- [ ] Run format, clippy, workspace tests, official golden/hash checks, training replay, invalid replay, verifier acceptance, and loader checks.
- [ ] Review every core file and confirm no forbidden search logic exists.

