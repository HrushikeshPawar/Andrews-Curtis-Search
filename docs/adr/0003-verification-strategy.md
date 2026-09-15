# ADR 0003: Layered verification

Status: accepted

Verification has three layers: `ac-core` replay and exact-target checking,
`ac-cli replay` for local use, and the official SAIR verifier as an external
process. Shared orientation fixtures, official move-spec metadata, golden
vectors, and the 424 official training certificates provide cross-checks.
An experiment record is versioned JSON and contains provenance, limits,
metrics, certificate, and verification status.
