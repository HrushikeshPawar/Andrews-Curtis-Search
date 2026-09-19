# Part A — ACSolverX pretrained-checkpoint probe

Reconnaissance only. No training. No SAIR certificates were emitted.

## Setup

- Upstream: `Math-AI-Caltech/ACSolverX`, released `ppo_checkpoints/610model` step 1000.
- Adapter: `scripts/acsolverx_probe.py`.
- Inference: JAX CPU greedy-argmax (no GPU). Official `beam_search.py` uses beam 1024; that is not practical on CPU, so this probe uses the same packed action head with greedy decode, `max_steps=50`, batch 64.
- Length cap: each relator padded to `L=24` (the checkpoint packing constant). Longer relators are incompatible.
- Triviality is **ACSolverX's**: two nonzero letters. Not the SAIR exact target `[[1],[2]]`.
- Starts are the exact SAIR relators, padded. No relator swap/invert canonicalization.

Implementation commit: `29b690e31bd93c2a9542dac86256805defc5b8ef`.

## Command

```bash
export SAIR_ACC_ROOT=/path/to/sair-acc
/tmp/acsolverx-venv/bin/python scripts/acsolverx_probe.py \
  --sair-root "$SAIR_ACC_ROOT" \
  --acsolverx-root /path/to/ACSolverX \
  --max-steps 50 --batch-size 64 \
  --out results/acsolverx_probe.json
```

Wall time: 479 s on JAX CPU.

## Counts

| quantity | n |
|---|---|
| scored ordinary-AC cases | 10,115 |
| compatible (`max relator length ≤ 24`) | 9,334 |
| incompatible | 781 |
| current scored inventory (S3.1 814) | 814 |
| compatible ∩ inventory | 755 |
| ACSolverX-trivial under greedy-argmax | 630 |
| overlap with the 814 inventory | 280 |
| **genuinely new vs the 814** | **350** |
| new IDs also in the frozen 500 residue slice | 17 |

Machine-readable artifact: `results/acsolverx_probe.json` (packed S-move paths retained).

## Stop condition

350 new ACSolverX-trivials is not negligible as a **model-coverage** signal. Work on the checkpoint itself stops here: no fine-tune, no beam-1024 rerun, no conversion of the 350 paths into official primitive certificates in this task.

Those paths are **not** competition solutions:

- destination is any two length-1 relators, not exact `[[1],[2]]`
- ACSolverX applies free Booth rotation
- stored actions are packed S-moves, not official ids `0..13`

Compiling them later would go through the S4.3 witnessed substitution constructor, then a short exact-target cleanup. That is a follow-up, not this probe.

## Recommendation

Do not scale ACSolverX training or GPU beam search. Keep the 350 trajectories on disk. Prefer the in-repo witnessed substitution search (Part B) for actual SAIR certificates.
