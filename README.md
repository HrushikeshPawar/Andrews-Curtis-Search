# andrew-curtis-search

Local Rust workspace for exact-state ordinary Andrews–Curtis search. The
repository is prepared through the search infrastructure milestone; exact-state
BFS is intentionally not implemented.

## Pinned semantics

- Official checkout: set `SAIR_ACC_ROOT` to the local SAIR repository.
- Pinned upstream commit: `a0fd6e6f52d82c93ccc06ab91d81e8fa3678256e`.
- Move spec: `ac-r2-v1`.
- Move-spec hash: `sha256:e13f57d82b5db1a79c1bd9d5ee1f6665ac4d96a634a3656ea20c0ada82e07643`.
- Exact target: `[[1], [2]]`.

The immutable fixtures under `fixtures/v1/` came from the orientation/scratch
repository. The vendored official files are limited to the move spec, golden
vectors, training data, and manifests; the full upstream repository is not
copied into this project.

## Commands

```bash
cargo fmt --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
SAIR_ACC_ROOT=/path/to/sair-acc ./scripts/run-official-verifier.sh --golden "$PWD/vendor/sair/verifier/data/golden_vectors.json"
```

Replay a local presentation/certificate pair:

```bash
cargo run -p ac-cli -- replay --start start.json --certificate certificate.json
```

`ac solve` currently reports that exact-state BFS is the next milestone. Put
the implementation in `crates/ac-search/src/lib.rs`, in
`solve_exact_state_bfs`; do not add canonicalization or another search method.

For later local benchmarks, use a release build and optionally set
`RUSTFLAGS="-C target-cpu=native"` only for that shell invocation. The default
release profile remains portable.
