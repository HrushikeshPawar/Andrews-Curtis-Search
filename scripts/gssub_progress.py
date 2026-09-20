#!/usr/bin/env python3
"""Live progress for an 8-way s3_sweep gssub run.

Default: the untouched 3689-case tail (`gssubtail`).

  python scripts/gssub_progress.py
  python scripts/gssub_progress.py --watch 5
"""
from __future__ import annotations

import argparse
import json
import os
import re
import sys
import time
from pathlib import Path

LINE = re.compile(
    r"^(ac-\d+)\s+start_len=(\d+)\s+(solved|limit|exhausted)\s+"
    r"cert_len=(\S+)\s+gen=(\d+)\s+uniq=(\d+)\s+ms=(\d+)"
)
REPO = Path(__file__).resolve().parents[1]


def parse_args() -> argparse.Namespace:
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--logdir", default="/tmp/gssubtail")
    p.add_argument("--pids", default="/tmp/gssubtail/pids")
    p.add_argument("--results", default=str(REPO / "results"))
    p.add_argument("--idfile", default=str(REPO / "results/s4_3_gssubrest_left.json"))
    p.add_argument("--tag", default="gssubtail")
    p.add_argument("--shards", type=int, default=8)
    p.add_argument("--watch", type=float, default=0, help="refresh seconds; 0 = print once")
    return p.parse_args()


def pid_alive(pid: int) -> bool:
    try:
        os.kill(pid, 0)
        return True
    except OSError:
        return False


def pid_rss_kb(pid: int) -> int:
    try:
        for line in Path(f"/proc/{pid}/status").read_text().splitlines():
            if line.startswith("VmRSS:"):
                return int(line.split()[1])
    except OSError:
        return 0
    return 0


def load_pids(path: Path) -> list[int]:
    if not path.exists():
        return []
    out = []
    for line in path.read_text().splitlines():
        line = line.strip()
        if line.isdigit():
            out.append(int(line))
    return out


def parse_log(path: Path) -> dict:
    solved = limit = exhausted = 0
    ms_sum = 0
    last = None
    bank = None
    if not path.exists():
        return {
            "finished": 0,
            "solved": 0,
            "limit": 0,
            "exhausted": 0,
            "ms_sum": 0,
            "last": None,
            "bank": None,
        }
    for line in path.read_text(errors="replace").splitlines():
        if line.startswith("bank built:"):
            try:
                bank = int(line.split()[2])
            except (IndexError, ValueError):
                pass
            continue
        m = LINE.match(line)
        if not m:
            continue
        rec = {
            "id": m.group(1),
            "start_len": int(m.group(2)),
            "status": m.group(3),
            "cert_len": m.group(4),
            "gen": int(m.group(5)),
            "uniq": int(m.group(6)),
            "ms": int(m.group(7)),
        }
        last = rec
        ms_sum += rec["ms"]
        if rec["status"] == "solved":
            solved += 1
        elif rec["status"] == "limit":
            limit += 1
        else:
            exhausted += 1
    return {
        "finished": solved + limit + exhausted,
        "solved": solved,
        "limit": limit,
        "exhausted": exhausted,
        "ms_sum": ms_sum,
        "last": last,
        "bank": bank,
    }


def cert_count(results: Path, tag: str, shards: int) -> int:
    n = 0
    for i in range(shards):
        p = results / f"s3_{tag}_scored_gssub_medium_shard_{i}.certs.txt"
        if not p.exists():
            continue
        for line in p.read_text().splitlines():
            if line.startswith("ac-") and ":" in line:
                n += 1
    return n


def json_done(results: Path, tag: str, shards: int) -> int:
    n = 0
    for i in range(shards):
        p = results / f"s3_{tag}_scored_gssub_medium_shard_{i}.json"
        if p.exists() and p.stat().st_size > 0:
            n += 1
    return n


def fmt_dur(seconds: float) -> str:
    if seconds < 0 or seconds != seconds:
        return "?"
    s = int(seconds)
    h, s = divmod(s, 3600)
    m, s = divmod(s, 60)
    if h:
        return f"{h}h{m:02d}m"
    return f"{m}m{s:02d}s"


def bar(frac: float, width: int = 28) -> str:
    frac = max(0.0, min(1.0, frac))
    filled = int(round(frac * width))
    return "[" + "#" * filled + "-" * (width - filled) + "]"


def render(args: argparse.Namespace) -> str:
    logdir = Path(args.logdir)
    results = Path(args.results)
    idfile = Path(args.idfile)
    total = 0
    if idfile.exists():
        total = len(json.loads(idfile.read_text()))
    pids = load_pids(Path(args.pids))
    lines = []
    now = time.time()
    start = None
    if Path(args.pids).exists():
        start = Path(args.pids).stat().st_mtime
    finished = solved = limit = exhausted = ms_sum = 0
    bank = None
    rss = 0
    alive = 0
    lines.append(f"gssub {args.tag}  shards={args.shards}  total={total}  {time.strftime('%H:%M:%S')}")
    lines.append("")
    hdr = f"{'sh':>2} {'pid':>7} {'st':>4} {'done':>5} {'sol':>4} {'lim':>4} {'last':>10} {'ms':>6} {'rss':>6}"
    lines.append(hdr)
    lines.append("-" * len(hdr))
    for i in range(args.shards):
        st = parse_log(logdir / f"shard_{i}.log")
        finished += st["finished"]
        solved += st["solved"]
        limit += st["limit"]
        exhausted += st["exhausted"]
        ms_sum += st["ms_sum"]
        if st["bank"] is not None:
            bank = st["bank"]
        pid = pids[i] if i < len(pids) else 0
        running = pid_alive(pid) if pid else False
        if running:
            alive += 1
            rss += pid_rss_kb(pid)
        last = st["last"]
        last_id = last["id"] if last else "-"
        last_ms = f"{last['ms']/1000:.0f}s" if last else "-"
        lines.append(
            f"{i:2d} {pid or 0:7d} {'run' if running else 'dead':>4} "
            f"{st['finished']:5d} {st['solved']:4d} {st['limit']:4d} "
            f"{last_id:>10} {last_ms:>6} {pid_rss_kb(pid)/1024:5.0f}M"
        )
    lines.append("")
    frac = (finished / total) if total else 0.0
    elapsed = (now - start) if start else 0.0
    rate = (finished / elapsed) if elapsed > 0 else 0.0
    remain = max(total - finished, 0)
    eta = (remain / rate) if rate > 0 else float("nan")
    mean_s = (ms_sum / finished / 1000.0) if finished else 0.0
    hit = (100.0 * solved / finished) if finished else 0.0
    certs = cert_count(results, args.tag, args.shards)
    dumps = json_done(results, args.tag, args.shards)
    lines.append(f"{bar(frac)} {finished}/{total}  {100*frac:5.1f}%")
    lines.append(
        f"solved={solved}  limit={limit}  exhausted={exhausted}  "
        f"hit={hit:.2f}%  live_certs={certs}  json_dumps={dumps}/{args.shards}"
    )
    lines.append(
        f"alive={alive}/{args.shards}  rss={rss/1024/1024:.1f}G  bank={bank}  "
        f"mean={mean_s:.1f}s/case  elapsed={fmt_dur(elapsed)}  eta={fmt_dur(eta)}"
    )
    lines.append(f"inventory if all live certs verify: 1271 + {certs} = {1271 + certs}")
    return "\n".join(lines) + "\n"


def main() -> int:
    args = parse_args()
    if args.watch and args.watch > 0:
        while True:
            text = render(args)
            sys.stdout.write("\033[H\033[J" + text)
            sys.stdout.flush()
            time.sleep(args.watch)
    else:
        sys.stdout.write(render(args))
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except KeyboardInterrupt:
        raise SystemExit(0)
