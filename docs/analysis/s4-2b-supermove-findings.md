# S4.2b supermove findings (first formulation)

Two formulations of witnessed substitution supermoves were implemented and
measured against the frozen 50-case residue pilot (same ids, same medium
budget, equal primitive-work accounting: one macro costs its witness length).

## What was implemented

`crates/ac-search/src/supermove.rs` generates composite proposals from real
primitive witnesses:

- rotate the target relator and/or the source relator by a short conjugator
  word (suffix or inverse prefix of that relator, so the conjugation is a
  length-neutral cyclic rotation), then
- multiply the (rotated) source into the target, optionally inverted.

Each proposal carries the exact successor, the full primitive witness, and the
signed net length change. Proposals are ranked by letters cancelled per witness
move; a small quota of length-non-decreasing proposals is kept as setup moves.
`propose_all` exposes the raw family for measurement.

## Measurements

**Residue frontier is uncancellable.** On real residue starts the rotation
family finds match lengths of 1-2 letters: net length change is `|appended| − 2m`,
so nothing cancels (best candidate on ac-00001 adds 5 letters; 36 candidates at
`max_conjugator = 1`, zero with positive cancellation; even at
`max_conjugator = 2` with 100 candidates the best is −1). Sampled across the
frozen runs, macros cancelled ~0.03 letters per expansion.

**Known corridors are cancellable.** Sampling 2,000 banked atlas states (every
269th of the 539,769): at `max_conjugator = 1`, 24.1% have at least one
cancelling supermove (0.58 cancelled letters/state, best cancellations up to
+14); at `max_conjugator = 2`, 40.9% (1.25 letters/state). So
cancellation-rich states exist on real solution paths while the residue
frontier has none: the asymmetry is not a tuning artifact.

**Pilot result (50 cases, medium budget).** All arms solve the same single
bank hit and nothing else; the macro arm explores 30% fewer distinct states per
primitive unit:

| arm | solved | macros | witness moves | macro share of budget | cancelled letters (per macro) |
|---|---|---|---|---|---|
| atlas-greedy (frozen) | 1/50 | — | — | 0 | — |
| supermove v1 (source rotations only) | 1/50 | 5.9M | 11.5M | 23.5% | 270,808 (0.05) |
| supermove v2 (rotations + un-rotation) | 1/50 | 4.5M | 20.2M | 41.2% | 111,762 (0.025) |
| supermove v3 (rotations, no un-rotation) | 1/50 | 5.6M | 15.3M | 31.3% | 719,439 (0.128) |

v3 (the corrected family) cancels ~7x more per macro than v1, but still finds
no new coverage: single-round cancellation is too rare per primitive charged.

**Full 500-case run (the promotion criterion).** At the same medium budget:

- supermove v3: **20/500** solved; frozen atlas-greedy: **19/500**. One new
  case (`ac-00380`), no losses.
- Every solve in both arms is a bank hit: the search reaches a banked state and
  completes with the stored suffix.
- `ac-00380` was reached after 9,210 generated primitives (4,947 unique states,
  504 expansions), while the baseline explored 633,440 unique states without
  hitting it. But the run's 1,011 emitted macros cancelled only **9 letters**
  in total: the gain is traversal perturbation by the macro arm (31.2% of the
  500-case budget went to witness moves), not cancellation power.
- Across the run: 55.2M macros, 0.121 cancelled letters per macro,
  `unique/generated` 0.443 vs 0.640 for the baseline (fewer distinct states
  per primitive).

The criterion is technically met (+1 coverage at equal primitive accounting),
but the effect is marginal and the mechanism unclear: the cancellation count
is far too small to explain the solve.

**Ablation: cancelling macros only (`SUBST_KEEP_INC=0`).** Same 500 cases,
same budget, fillers removed so a macro is emitted only when it cancels:

| arm | solved | macros | witness share of budget | cancelled / macro | novel cases |
|---|---|---|---|---|---|
| atlas-greedy (frozen) | 19/500 | — | — | — | — |
| supermove v3, fillers kept | 20/500 | 55.2M | 31.2% | 0.121 | ac-00380 |
| **supermove v3, cancelling only** | **22/500** | 5.4M | **2.81%** | **1.674** | ac-00364, ac-00380, ac-00557 |

No losses in either direction: the solve sets are nested
19 ⊂ 20 ⊂ 22. With fillers removed the macro overhead drops to 2.8% of the
budget, `unique/generated` returns to baseline (0.622 vs 0.640), and every
cancellation event is real (5.4M macros, 9.1M letters).

The three new cases all show macro activity before the bank hit:

- `ac-00380`: solved at 9,957 generated / 764 expansions after 4 macros and 9
  cancelled letters — a corridor opened early;
- `ac-00364`: solved at 961,419 / 1M after 5,342 macros;
- `ac-00557`: solved at 909,487 / 1M after 15,380 macros.

Two of the three are saved near the budget cap; all are bank hits (no new
certificate length). So cancelling supermoves are the effective component and
filler macros were actively harmful — they burned 28% of the budget for a
worse result.

## Why single-round macros are inert (structural)

Conjugation by a suffix or inverse prefix of a relator is a *length-neutral*
rotation. Therefore a single-round macro is a 2-3 move witness whose
intermediate states sit at the same total length as the source state. A
length-greedy search expands all states of a given length before any longer
one, so it already visits the rotated intermediate state at the same tier;
the macro only shortcuts ordering, it does not add reachability. The pilot
confirms this: the macro arm explores fewer distinct states and solves the
same cases.

## Implication

The ablation resolves the picture: cancellation-producing supermoves are the
effective component. Removing filler proposals *improves* coverage
(19 baseline → 20 all macros → 22 cancelling-only) while cutting macro
overhead from 31% to 2.8% of the budget. The gain is still modest (+3/500,
all bank hits) and deterministic single runs can be chaotic, so a
dose-response check (`max_conjugator = 2`, cancelling only) is the next step
before promotion.

The reachability argument still bounds what this family can do: rotations are
length-neutral, so single-round macros only reroute the traversal at equal
total length. The larger opportunity remains a two-round generator — compose
the best length-increasing setup proposals with the cancelling proposals
available at their successors, keep only net-negative compositions, and charge
the summed witness. That is the only shape that adds reachability that
length-greedy deprioritizes.

## Reproduction

```bash
export SAIR_ACC_ROOT=/path/to/sair-acc
cargo build --release -p ac-search --examples
./target/release/examples/supermove_stats ac-00001 ac-00018   # per-state detail
./target/release/examples/supermove_stats bank:2000           # corridor sample
./target/release/examples/s3_sweep 0 1 subst medium scored tag idfile.json
```
