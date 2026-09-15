# ADR 0001: Pin ordinary AC semantics to SAIR

Status: accepted

Ordinary rank-2 AC uses SAIR `ac-r2-v1`, its 14 move IDs, signed generator
encoding, free reduction, inverse table, and exact ordered target `[[1], [2]]`.
The accepted move-spec hash is
`sha256:e13f57d82b5db1a79c1bd9d5ee1f6665ac4d96a634a3656ea20c0ada82e07643`.

The Rust kernel is behavior-faithful to the trusted orientation implementation.
It does not cyclically reduce, swap relators, quotient inversions, or apply
Booth canonicalization.
