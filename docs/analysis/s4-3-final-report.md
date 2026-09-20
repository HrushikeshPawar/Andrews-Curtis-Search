# S4.3 final report — ACSolverX probe, GSSUB search, SAIR submissions

Team `ACC01-T00082`. Branch `hrushikesh/gssub-action-space`.  
Ordinary AC only (`ac-r2-v1`, target `[[1],[2]]`). Frozen primitive greedy/beam unchanged.

## The two tasks

### Part A — cheap ACSolverX pretrained probe

Released `610model`, JAX CPU greedy-argmax, `L=24`, `max_steps=50`. No training.

| quantity | n |
|---|---|
| scored ordinary-AC | 10,115 |
| compatible (`relator ≤ 24`) | 9,334 |
| ACSolverX-trivial (two nonzero letters) | 630 |
| overlap with S3.1 814 | 280 |
| new vs 814 | **350** |

Those 350 are packed S-moves under ACSolverX triviality, **not** SAIR certificates. They were **not** submitted. Artifact: `results/acsolverx_probe.json`.

### Part B — witnessed substitution search (GSSUB)

Exact state → cancelling substitution (rotate, optional invert, left/right multiply, restore other relator, cyclic-reduce) → exact state, with an official primitive witness. Search over **macro edges only**, bank probing on, reverse ball off, medium budget `max_work = 1_000_000`.

| slice | cases | solved | hit | official |
|---|---|---|---|---|
| Frozen 500-case residue vs atlas-19 | 500 | **39** | 7.8% | 39/39 |
| vs S4.2b subst K=2 (23) | — | +16 new IDs | — | yes |
| Recovered from killed 9,262-run logs | 418 | 418 recert | — | 219+199 = 418/418 |
| Untouched tail | 3,689 | **320** | 8.7% | 320/320 |
| **GSSUB unique** | — | **777** | — | 777/777 |

All GSSUB solves were **bank hits**. Certificates are long reconstructed prefixes plus stored suffixes (median hundreds to thousands of primitives).

Local scored inventory after GSSUB: **814 (S3.1) + 777 = 1,591**.

## SAIR submissions (this is what scores)

Nothing counts until it is uploaded. Four Discovery Track AC batches, all **complete**:

| id | when (UTC) | n | official ok |
|---|---|---|---|
| 1469 | 2026-09-20 06:11:09 | 500 | 500/500 |
| 1470 | 2026-09-20 06:11:35 | 500 | 500/500 |
| 1471 | 2026-09-20 06:11:36 | 356 | 356/356 |
| 1495 | 2026-09-20 11:03:39 | 235 | 235/235 |
| **total unique `ac-*`** | | **1,591** | **1,591/1,591** |

Split into four files because the platform cap is **500 solutions/file**. Training `ms-train-*` IDs were excluded.

Provenance of the 1,591 uploaded IDs:

| source | n |
|---|---|
| S3.1 greedy/beam | 814 |
| GSSUB 500-slice | 39 |
| GSSUB recovered recert | 219 |
| GSSUB 199 recert | 199 |
| GSSUB tail (new vs first upload) | 320 total tail, of which 235 in 1495 and 85 already in 1469–1471 |
| **unique** | **1,591** |

Uploaded length stats: min **8**, median **769**, p90 **3,549**, max **4,616**. 112 certs have length ≤ 50; 683 have length ≥ 1,000.

## What the leaderboard did with those submissions

AC board snapshot **2026-09-20 12:50 UTC** (after all four batches):

| | |
|---|---|
| Rank | **26 / 34** teams |
| Score | **0.0000** (was 0.0001 at 06:30) |
| `currentBestCount` | **58** |

Scoring rule: on each challenge, the \(k\) teams tied for **shortest** verified solution each get \(2^{1-k}\); longer valid certs get **0**.

Interpretation:

- **1,591 accepted ≠ 1,591 points.** Only 58 IDs currently share a shortest record.
- Those 58 are almost certainly the **short S3 greedy** certs (some length 8), **heavily tied** with many other teams, so each is worth ~\(10^{-5}\)–\(10^{-4}\).
- The 777 GSSUB certs (median ~1.5k–2k moves) verified on SAIR but are **not competitive length** against the top of the board (rank 1 holds 4,285 shortest records, score 2456).
- Rank 26 is a real place on the board. The score is the length-sharing rule, not a verifier failure.

Top of the same snapshot: 冰冻野鸡大学 2456.17 (4285 bests), AC-Solver =D 2444.59 (4170), then a steep drop.

## Findings vs the original decision gate

**Part A:** 350 new ACSolverX-trivials is a real model-coverage signal. Stop training/beam. Do not convert those S-move paths until GSSUB production is finished. **Abandon ACSolverX as a scoring method.**

**Part B:** GSSUB **does** produce new official SAIR certificates at scale (777 new vs the 814). On the frozen 500 it doubled atlas-greedy (19 → 39). On the remaining unsolved corpus it added 418 + 320. **SCALE was correct.** Medium budget is enough for breadth.

**Submissions:** uploading the inventory **does** put the team on the AC leaderboard, but **does not** buy points until certificates are shortest. Next scoring work is (1) unique shortest on IDs the top two have not solved, or (2) shortening the 58 ties — not another wider primitive beam, and not a larger budget on every remaining fail.

## Commands / artifacts

```text
results/acc_submission_ac_part{01,02,03}_of_03.txt
results/s4_3_gssubtail_new_vs_upload.txt   # batch 1495
results/acc_submission_ac_manifest.json
docs/analysis/s4-3-gssub-findings.md
docs/analysis/s4-3-acsolverx-probe.md
```

Implementation: `crates/ac-search/src/substitution.rs`. Solver tag `gssub`.
