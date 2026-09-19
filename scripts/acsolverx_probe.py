#!/usr/bin/env python3
"""Part A: ACSolverX pretrained-checkpoint probe on scored SAIR cases.

Feeds rank-2 scored presentations whose relators fit L=24 into the released
610model Dual-Ring policy. Does not train. Triviality is ACSolverX's
(two nonzero letters), not the SAIR exact target [[1],[2]].

Usage (from anywhere; paths can be overridden):

  python scripts/acsolverx_probe.py \\
      --sair-root "$SAIR_ACC_ROOT" \\
      --acsolverx-root "/path/to/ACSolverX" \\
      --out results/acsolverx_probe.json
"""
from __future__ import annotations

import argparse
import json
import os
import sys
import time
from pathlib import Path


L = 24  # ACSolverX training / beam packing constant


def parse_args() -> argparse.Namespace:
    repo = Path(__file__).resolve().parents[1]
    p = argparse.ArgumentParser()
    p.add_argument(
        "--sair-root",
        default=os.environ.get("SAIR_ACC_ROOT", ""),
        help="official SAIR checkout (needs competition/tools/verifier/data/manifest.json)",
    )
    p.add_argument(
        "--acsolverx-root",
        default="",
        help="upstream ACSolverX checkout with ppo_checkpoints/610model",
    )
    p.add_argument("--ckpt-path", default="610model")
    p.add_argument("--ckpt-step", type=int, default=-1)
    p.add_argument("--max-steps", type=int, default=50)
    p.add_argument("--batch-size", type=int, default=64)
    p.add_argument(
        "--inventory",
        action="append",
        default=[],
        help="submission files whose ids count as already solved (repeatable)",
    )
    p.add_argument("--out", default=str(repo / "results" / "acsolverx_probe.json"))
    p.add_argument(
        "--compatible-only",
        action="store_true",
        help="count compatible cases and exit (no JAX / checkpoint)",
    )
    p.add_argument(
        "--max-cases",
        type=int,
        default=0,
        help="cap compatible cases after inventory tagging (0 = all)",
    )
    return p.parse_args()


def load_scored(sair_root: Path) -> list[dict]:
    manifest = sair_root / "competition/tools/verifier/data/manifest.json"
    data = json.loads(manifest.read_text())
    out = []
    for c in data["challenges"]:
        if c.get("move_spec_version") != "ac-r2-v1":
            continue
        if not c.get("scored"):
            continue
        r0, r1 = c["initial_relators"]
        out.append(
            {
                "id": c["challenge_id"],
                "r0": [int(x) for x in r0],
                "r1": [int(x) for x in r1],
            }
        )
    return out


def load_inventory(paths: list[str], repo: Path) -> set[str]:
    ids: set[str] = set()
    defaults = [
        repo / "results/s31_814_a.txt",
        repo / "results/s31_814_b.txt",
    ]
    files = [Path(p) for p in paths] if paths else defaults
    for path in files:
        for line in path.read_text().splitlines():
            line = line.strip()
            if not line or line.startswith("#") or ":" not in line:
                continue
            ids.add(line.split(":", 1)[0].strip())
    return ids


def pad_relator(word: list[int], length: int = L) -> list[int]:
    if len(word) > length:
        raise ValueError("relator exceeds L")
    return word + [0] * (length - len(word))


def pad_presentation(r0: list[int], r1: list[int]) -> list[int]:
    return pad_relator(r0) + pad_relator(r1)


def is_compatible(r0: list[int], r1: list[int]) -> bool:
    return len(r0) <= L and len(r1) <= L


def acsolverx_trivial(flat: list[int]) -> bool:
    return sum(1 for x in flat if x != 0) == 2


def numpy_s_move(x, action, max_length: int = L):
    """NumPy port of ACSolverX envs.ac_moves.s_move (no Booth skip on equal)."""
    import numpy as np

    x = np.array(x, dtype=np.int8).copy()
    i, r, k1, k2 = (int(action[0]), int(action[1]), int(action[2]), int(action[3]))

    def rel(arr, idx):
        return arr[idx * max_length : (idx + 1) * max_length]

    def set_rel(arr, idx, word):
        arr[idx * max_length : (idx + 1) * max_length] = word
        return arr

    def nonzero(word):
        return word[word != 0]

    def invert_word(word):
        nz = nonzero(word)
        out = np.zeros_like(word)
        if nz.size:
            out[: nz.size] = -nz[::-1]
        return out

    def rotate_word(word, k):
        nz = nonzero(word)
        out = np.zeros_like(word)
        if nz.size:
            k_mod = int(k) % int(nz.size)
            rot = np.concatenate([nz[k_mod:], nz[:k_mod]])
            out[: nz.size] = rot
        return out

    def concat_cancel(a, b):
        an, bn = nonzero(a), nonzero(b)
        n = 0
        while n < an.size and n < bn.size and an[-(n + 1)] == -bn[n]:
            n += 1
        body = np.concatenate([an[: an.size - n] if n else an, bn[n:]])
        # cyclic reduce
        while body.size >= 2 and body[0] == -body[-1]:
            body = body[1:-1]
        if body.size > max_length:
            return None
        # Booth lex-min rotation (ACSolverX s_move does this; Part A only).
        if body.size:
            doubled = np.concatenate([body, body])
            best = 0
            for i in range(1, body.size):
                if tuple(doubled[i : i + body.size]) < tuple(doubled[best : best + body.size]):
                    best = i
            body = doubled[best : best + body.size]
        out = np.zeros(max_length, dtype=np.int8)
        if body.size:
            out[: body.size] = body
        return out

    x_comp = x.copy()
    if r == 1:
        set_rel(x_comp, 1, invert_word(rel(x_comp, 1)))
    r0 = rotate_word(rel(x_comp, 0), k1)
    r1 = rotate_word(rel(x_comp, 1), k2)
    product = concat_cancel(r0, r1)
    if product is None or np.array_equal(product, r0):
        return x
    return set_rel(x.copy(), i, product)


def decode_action(sample: int, max_length: int = L):
    a = int(sample)
    k1 = (a // (4 * max_length)) + 1
    rem = a % (4 * max_length)
    k2_tmp = rem // 4
    ij = rem % 4
    i = ij // 2
    j = ij % 2
    k2 = k2_tmp * ((-1) ** j) - j
    return [i, j, k1, k2]


def run_greedy(acsolverx_root: Path, ckpt_name: str, ckpt_step: int, cases: list[dict],
               max_steps: int, batch_size: int) -> list[dict]:
    sys.path.insert(0, str(acsolverx_root))
    os.chdir(acsolverx_root)
    os.environ.setdefault("XLA_PYTHON_CLIENT_PREALLOCATE", "false")
    os.environ.setdefault("JAX_PLATFORMS", "cpu")

    import numpy as np
    import jax
    import jax.numpy as jnp
    import orbax.checkpoint as ocp
    from network import RelativeDualRingActorCritic

    jax.config.update("jax_default_matmul_precision", "float32")
    print(f"JAX devices: {jax.devices()}", flush=True)

    network = RelativeDualRingActorCritic(activation="gelu")
    rng = jax.random.PRNGKey(0)
    dummy = network.init(rng, jnp.zeros((1, 2 * L), dtype=jnp.int8))

    ckpt_abs = acsolverx_root / "ppo_checkpoints" / ckpt_name
    mngr = ocp.CheckpointManager(str(ckpt_abs), item_names=("params", "solve_data", "config"))
    step = mngr.latest_step() if ckpt_step < 0 else ckpt_step
    if step is None:
        raise SystemExit(f"No checkpoints at {ckpt_abs}")
    restored = mngr.restore(
        step,
        args=ocp.args.Composite(
            params=ocp.args.StandardRestore(dummy),
        ),
    )
    params = restored.params
    print(f"Restored {ckpt_name} step {step} (params only)", flush=True)

    @jax.jit
    def greedy_action(obs):
        pi, _value = network.apply(params, obs)
        return jnp.argmax(pi.logits, axis=-1)

    rows = []
    n = len(cases)
    t0 = time.time()
    for start in range(0, n, batch_size):
        batch = cases[start : start + batch_size]
        states = np.stack(
            [np.array(pad_presentation(c["r0"], c["r1"]), dtype=np.int8) for c in batch]
        )
        alive = np.ones(len(batch), dtype=bool)
        solved = np.zeros(len(batch), dtype=bool)
        path_len = np.full(len(batch), -1, dtype=np.int32)
        paths: list[list[int]] = [[] for _ in batch]
        for t in range(max_steps):
            if not alive.any():
                break
            obs = jnp.array(states)
            actions = np.asarray(greedy_action(obs))
            for b, case in enumerate(batch):
                if not alive[b]:
                    continue
                if acsolverx_trivial(states[b].tolist()):
                    solved[b] = True
                    path_len[b] = t
                    alive[b] = False
                    continue
                decoded = decode_action(int(actions[b]))
                nxt = numpy_s_move(states[b], decoded)
                if nxt is None or np.array_equal(nxt, states[b]):
                    alive[b] = False
                    continue
                states[b] = nxt
                paths[b].append(int(actions[b]))
                if acsolverx_trivial(states[b].tolist()):
                    solved[b] = True
                    path_len[b] = t + 1
                    alive[b] = False
        for b, case in enumerate(batch):
            rows.append(
                {
                    "id": case["id"],
                    "solved_acsolverx": bool(solved[b]),
                    "path_length": int(path_len[b]),
                    "path": paths[b] if solved[b] else [],
                    "final_nonzero": int(np.count_nonzero(states[b])),
                    "in_inventory": bool(case.get("in_inventory")),
                }
            )
        done = start + len(batch)
        elapsed = time.time() - t0
        n_sol = sum(1 for r in rows if r["solved_acsolverx"])
        print(
            f"progress {done}/{n} solved_so_far={n_sol} elapsed={elapsed:.1f}s",
            flush=True,
        )
    return rows


def main() -> int:
    args = parse_args()
    repo = Path(__file__).resolve().parents[1]
    if not args.sair_root:
        raise SystemExit("set --sair-root or SAIR_ACC_ROOT")
    sair_root = Path(args.sair_root)
    scored = load_scored(sair_root)
    inventory = load_inventory(args.inventory, repo)
    compatible = []
    for c in scored:
        if is_compatible(c["r0"], c["r1"]):
            c["in_inventory"] = c["id"] in inventory
            compatible.append(c)
    n_compat = len(compatible)
    n_compat_inv = sum(1 for c in compatible if c["in_inventory"])
    if args.max_cases and args.max_cases > 0:
        compatible = compatible[: args.max_cases]
    summary = {
        "format_version": 1,
        "part": "A",
        "ckpt_path": args.ckpt_path,
        "max_length": L,
        "max_steps": args.max_steps,
        "scored_total": len(scored),
        "compatible": n_compat,
        "incompatible": len(scored) - n_compat,
        "inventory_size": len(inventory),
        "compatible_in_inventory": n_compat_inv,
        "compatible_unsolved_inventory": n_compat - n_compat_inv,
        "inference": None,
        "rows": [],
    }
    print(
        f"scored={len(scored)} compatible_L{L}={n_compat} "
        f"inventory={len(inventory)} compatible_in_inventory="
        f"{summary['compatible_in_inventory']}",
        flush=True,
    )
    if args.compatible_only:
        Path(args.out).parent.mkdir(parents=True, exist_ok=True)
        Path(args.out).write_text(json.dumps(summary, indent=2) + "\n")
        print(f"wrote {args.out}")
        return 0

    acx = Path(args.acsolverx_root) if args.acsolverx_root else None
    if acx is None or not acx.exists():
        raise SystemExit("need --acsolverx-root pointing at the ACSolverX checkout")
    t0 = time.time()
    rows = run_greedy(
        acx, args.ckpt_path, args.ckpt_step, compatible, args.max_steps, args.batch_size
    )
    solved = [r for r in rows if r["solved_acsolverx"]]
    new = [r for r in solved if not r["in_inventory"]]
    overlap = [r for r in solved if r["in_inventory"]]
    summary["inference"] = {
        "backend": "jax-cpu-greedy-argmax",
        "batch_size": args.batch_size,
        "elapsed_s": time.time() - t0,
        "reached_acsolverx_trivial": len(solved),
        "overlap_inventory": len(overlap),
        "genuinely_new": len(new),
        "new_ids": [r["id"] for r in new],
        "overlap_ids": [r["id"] for r in overlap],
    }
    summary["rows"] = rows
    Path(args.out).parent.mkdir(parents=True, exist_ok=True)
    Path(args.out).write_text(json.dumps(summary, indent=2) + "\n")
    print(
        f"ACSolverX trivial {len(solved)}/{len(compatible)}; "
        f"overlap {len(overlap)}; new {len(new)} {summary['inference']['new_ids'][:20]}"
    )
    print(f"wrote {args.out}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
