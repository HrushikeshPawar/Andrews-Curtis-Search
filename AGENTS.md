# Agent boundaries

This repository is a local Rust workspace for ordinary rank-2 AC semantics and
future exact-state BFS. `ac-core` is the mathematical authority; any semantic
change must be checked against the pinned official SAIR move specification.
`ac-search` contains interfaces and metrics only. `ac-cli` stays a thin local
adapter around replay, external verification, data loading, and submission I/O.

Agents may change workspace plumbing, serialization, loaders, CLI plumbing,
tests, documentation, local benchmark plumbing, and semantics-preserving
refactors.

Without explicit permission, agents must not implement BFS, greedy, beam,
A*/IDA*, bidirectional search, search canonicalization, supermoves, certificate
shortening, Stable AC search, learned heuristics, or RL. Do not add CI, Docker,
PyO3, an in-repo Python package, JAX, or PyTorch.

The official Python verifier is external and selected with `SAIR_ACC_ROOT`.
Do not reimplement it in Rust.
