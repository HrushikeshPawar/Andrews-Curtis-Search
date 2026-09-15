# ADR 0002: Rust ownership and Python boundary

Status: accepted

Rust owns words, presentations, typed moves, certificates, search metrics, and
local CLI plumbing. The official SAIR Python checkout remains an external
dependency selected by `SAIR_ACC_ROOT`; this repository invokes its verifier as
a subprocess and does not create a Python package or duplicate verifier logic.
