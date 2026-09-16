# S4.2a quotient instrumentation findings

Analysis-only experiment. Nothing in `ac-core` semantics or any frozen solver
changed; the probe attaches a discovery observer to `solve_bank_greedy`.

## What was measured

`crates/ac-search/src/quotient.rs` projects presentations into

- the class-2 nilpotent quotient `H = F2 / (gamma_3(F2) * F2^p)`, elements
  `x^a y^b [x,y]^c`, abstract state `(relator_0, relator_1) in H x H`, so
  `p^6` abstract states, and
- its abelianization (`p^4` states).

Every abstract distance is an exhaustive multi-source BFS distance, and every
official move projects onto the matching abstract move (unit-tested against
all 14 move ids), so the abstract distance is an admissible lower bound on
exact primitive distance.

`examples/quotient_probe.rs` projects four populations per phase:

- `atlas`: the 539,769 banked states with their shortest known suffix;
- `starts`: the 500 residue starts and the 1,238 solved starts;
- `traj`: all 644,122 intermediate states of the 1,238 verified certificates;
- `failed`: every 16th state discovered by the frozen medium-budget
  bank-greedy on the 500-case residue slice (solved runs kept separate).

## Validation (the tables are trustworthy)

- Move commutation holds for all 14 moves on random states at `p = 2, 3, 5`.
- `abstract distance <= known suffix length` on all 539,769 atlas states:
  **0 violations**.
- Same bound against remaining certificate length on all 644,122 trajectory
  states: **0 violations**.
- Abelian distance never exceeds class-2 distance: 0 violations (by construction,
  checked in unit tests).

## Results

Sensitivity sweep over the prime (occupancy = distinct atlas signatures held):

| p | abstract states | atlas occupancy | atlas mean qdist | residue starts mean qdist (p50) | traj steps decreasing | Spearman(qdist, suffix) | eta^2(signature, suffix) |
|---|---|---|---|---|---|---|---|
| 3 | 729 | 59.3% | 3.28 | 3.28 (3) | 22.7% | -0.016 | 0.013 |
| 5 | 15,625 | 37.5% | 4.68 | 4.76 (5) | 24.5% | -0.022 | 0.036 |
| 7 | 117,649 | 21.0% | 5.67 | 5.83 (6) | 25.2% | +0.014 | 0.100 |
| 11 | 1,771,561 | 4.5% | 6.56 | 7.19 (7) | 26.6% | +0.054 | 0.239 |
| 13 | 4,826,809 | 2.1% | 6.73 | 7.58 (8) | 27.2% | +0.068 | 0.291 |

Trajectory motion is a symmetric walk: at `p = 5`, of 642,884 transitions,
24.5% decrease, 52.0% stay flat, 23.6% increase, mean drift `-0.009` per step.
At `p = 13` the drift is still only `-0.014`.

Distance to the nearest atlas signature is also saturated: 497/500 residue
starts sit at abstract distance 0 from an atlas signature at `p = 5`; even at
`p = 13`, 352/500 are at 0 and the maximum is 3.

## Conclusions

1. **Trajectory monotonicity: no.** A quotient potential does not descend
   along verified solutions; it is a random walk with no usable drift.
2. **Equal-length separation: no.** Joining the failed-search sample
   (19.4M sampled discoveries, 481 failed runs) and the verified-trajectory
   states per exact total relator length gives indistinguishable
   distributions: weighted mean qdist `4.714` (failed, n=17.4M) vs `4.677`
   (trajectory, n=611k), delta `+0.037`; medians are 5 everywhere and the
   p10/p90 bands coincide. The quotient does not separate productive from
   dead mid-game states at equal length.
3. **Atlas vs quotient space:** the atlas does not collapse the quotient at any
   tested prime (37.5% occupancy at `p = 5`, 21% at `p = 7`). Occupancy was
   not the failure mode.
4. **Suffix-length correlation: weak.** At `p = 5` (the planned prime) the
   signature explains ~3.6% of suffix-length variance and Spearman is ~-0.02.
   It grows with `p` (29% at `p = 13`) but suffix length is itself a noisy
   proxy for true distance, and the distance range stays tiny.
5. **Ball-7 post-mortem is neutral, not explained:** all 500 residue starts
   have `c2_dist <= 7`, but because the bound saturates at the quotient
   diameter, that says nothing about their exact distance. The zero ball-hit
   rate remains a purely exact-geometry result.

**Structural failure mode: lower-bound saturation.** The abstract graph
diameter is about 8 at `p = 5` and about 13 at `p = 13`, while the residue's
exact primitive distances are in the hundreds. Almost every mid-game state
sits on the plateau (at `p = 5`, 258,671 of 539,769 atlas states are at exactly
qdist 5), so the potential is near-constant across the region the search
actually explores. No finite homomorphic quotient escapes this: the bound is
limited by the quotient's diameter, which grows only logarithmically with the
abstract state count.

## Implications for S4.2

- Do **not** wire target-distance quotients into greedy/beam (candidates A/B):
  the potential cannot discriminate (Q1, Q2, Q4 all negative). The
  atlas-distance variant is dead for the same reason.
- The one surviving quotient use is diversity bucketing by signature
  (candidate C). It does not rely on the distance table, and `p = 5..7` is the
  right granularity (15k-118k signatures vs ~630k explored states per case, so
  a beam of 16 can be forced across genuinely different buckets). Test it
  inside the S4.2b experiment matrix as a beam variant, not as a separate
  milestone.
- The main bet is S4.2b: witnessed substitution supermoves, charged at
  primitive-witness cost.

## Reproduction

```bash
export SAIR_ACC_ROOT=/path/to/sair-acc
cargo build --release -p ac-search --example quotient_probe
./target/release/examples/quotient_probe atlas 5    # fast; also starts | traj
./target/release/examples/quotient_probe failed 5   # ~16 min at medium budget
```

Per-phase records: `results/s4_quotient_p{p}_{phase}.json`.
