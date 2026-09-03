#!/usr/bin/env python3
"""Paired A/B bench: one-grep (lexical/hybrid) vs rg vs zg on nixos-config copy.

Metrics per query: wall latency (best of 3), recall@3 (expected file hit),
hits returned. Index/embed wall time measured separately.
Set MODEL=minilm|arctic-m|gemma-300m to choose the embedding model.
"""
import json
import os
import shutil
import statistics
import subprocess
import sys
import tempfile
import time
from pathlib import Path

MODEL = os.environ.get("MODEL", "minilm")
# dev: tune here. held: verify here (overfit check). all: full table.
SPLIT = os.environ.get("SPLIT", "all")

ONE_GREP = Path("/Users/amitsheokand/dev/one-grep/target/release/one-grep")
if not ONE_GREP.exists():
    ONE_GREP = Path("/Users/amitsheokand/dev/one-grep/target/debug/one-grep")
CORPUS_SRC = Path("/Users/amitsheokand/dev/nixos-config")

QUERIES = [
    # (label, query, rg pattern, expected substring, difficulty 1-3)
    ("mlx server", "mlx language model server", "mlx-lm-server", "mlx-mac.nix", 1),
    ("rust setup", "rust toolchain installation setup", "rustup", "home-manager.nix", 2),
    ("launchd svc", "background service launchd agent", "launchd",
     "hosts/darwin/default.nix", 1),
    ("compactor", "compactor model adapter fusion", "compactor", "mlx-compactor.nix", 1),
    ("tmux keys", "tmux key bindings pane navigation", "select-pane", "home-manager.nix", 2),
    ("font cfg", "terminal font family size", "MesloLGS", "home-manager.nix", 2),
    ("ssh hosts", "ssh host configuration", "matchBlocks", "home-manager.nix", 1),
    ("stop gemma", "stop gemma lane and start compact lane", "stop gemma",
     "mlx-lane.nix", 2),
    # Call-chain questions (chain chunks: caller > calls > callee).
    ("chain apply", "what does apply_request call", "apply_request",
     "hipfire-profile-proxy.py", 2),
    ("chain callers", "which functions call apply_lane_defaults", "apply_lane_defaults",
     "hipfire-profile-proxy.py", 2),
]

# Paraphrased, low keyword overlap with targets (all difficulty 3).
CONCEPTS = [
    ("never both", "exclusive lane, never both at once", "mlx-lane.nix", 3),
    ("pretty prompt", "make the terminal prompt show git status with pretty colors",
     "home-manager.nix", 3),
    ("save session", "automatically preserve my terminal session every few minutes",
     "home-manager.nix", 3),
    ("no secrets", "where do I put tokens so they never get committed",
     "home-manager.nix", 3),
]

# Stratified dev/held split (by index within each set).
DEV_IDX = {"queries": {0, 1, 4, 5, 8}, "concepts": {0, 1}}


def run(cmd, **kw):
    return subprocess.run(cmd, capture_output=True, text=True, **kw)


def best_of_3(cmd, **kw):
    ts = []
    out = None
    for _ in range(3):
        t0 = time.perf_counter()
        out = run(cmd, **kw)
        ts.append(time.perf_counter() - t0)
    return min(ts), out


def hit_paths(stdout):
    """Count rendered hit headers (absolute path + line range)."""
    return [l for l in stdout.splitlines()
            if l.startswith("/") and ":" in l and "-" in l.split(":")[1][:12]]


def run_keyword_set(label, query, pattern, expected, ws):
    row = {"q": label, "expected": expected}
    lat, out = best_of_3(["rg", "-l", "--sort", "path", pattern, "."], cwd=ws)
    files = out.stdout.splitlines()
    row["rg_ms"] = round(lat * 1000, 1)
    row["rg_recall3"] = any(expected in f for f in files[:3])
    lat, out = best_of_3(
        [str(ONE_GREP), "query", query, "--path", str(ws), "--limit", "3"])
    row["lex_ms"] = round(lat * 1000, 1)
    row["lex_recall3"] = expected in out.stdout
    row["lex_n"] = len(hit_paths(out.stdout))
    lat, out = best_of_3(
        [str(ONE_GREP), "query", query, "--path", str(ws),
         "--limit", "3", "--hybrid"])
    row["hyb_ms"] = round(lat * 1000, 1)
    row["hyb_recall3"] = expected in out.stdout
    lat, out = best_of_3(
        [str(ONE_GREP), "query", query, "--path", str(ws),
         "--limit", "3", "--rerank"])
    row["rkh_ms"] = round(lat * 1000, 1)
    row["rkh_recall3"] = expected in out.stdout
    lat, out = best_of_3(
        ["zg", "query", "--human", query, "--limit", "3"], cwd=ws)
    row["zg_ms"] = round(lat * 1000, 1) if out.returncode == 0 else None
    row["zg_recall3"] = expected in out.stdout if out.returncode == 0 else None
    return row


def run_concept_set(label, query, expected, ws):
    row = {"q": label, "expected": expected}
    lat, out = best_of_3(
        [str(ONE_GREP), "query", query, "--path", str(ws), "--limit", "3"])
    row["lex_ms"] = round(lat * 1000, 1)
    row["lex_recall3"] = expected in out.stdout
    lat, out = best_of_3(
        [str(ONE_GREP), "query", query, "--path", str(ws),
         "--limit", "3", "--hybrid"])
    row["hyb_ms"] = round(lat * 1000, 1)
    row["hyb_recall3"] = expected in out.stdout
    lat, out = best_of_3(
        [str(ONE_GREP), "query", query, "--path", str(ws),
         "--limit", "3", "--rerank"])
    row["rkh_ms"] = round(lat * 1000, 1)
    row["rkh_recall3"] = expected in out.stdout
    lat, out = best_of_3(
        ["zg", "query", "--human", query, "--limit", "3"], cwd=ws)
    row["zg_ms"] = round(lat * 1000, 1) if out.returncode == 0 else None
    row["zg_recall3"] = expected in out.stdout if out.returncode == 0 else None
    return row


def main():
    tmp = Path(tempfile.mkdtemp(prefix="onegrep-bench-"))
    ws = tmp / "nixos-config"
    print(f"copy corpus -> {ws}", flush=True)
    shutil.copytree(CORPUS_SRC, ws, symlinks=True,
                    ignore=shutil.ignore_patterns(".git"))
    results = {"corpus": str(ws), "queries": []}

    t0 = time.perf_counter()
    r = run([str(ONE_GREP), "index", str(ws)])
    assert r.returncode == 0, r.stderr
    results["onegrep_index_s"] = round(time.perf_counter() - t0, 2)

    t0 = time.perf_counter()
    r = run([str(ONE_GREP), "embed", "--model", MODEL, str(ws)])
    assert r.returncode == 0, r.stderr[-500:]
    results["onegrep_embed_s"] = round(time.perf_counter() - t0, 2)
    results["model"] = MODEL

    zg_index_s = None
    r = run(["zg", "index", "--embedding", "local/potion-retrieval-32m"],
            cwd=ws, timeout=1200)
    if r.returncode == 0:
        # zg prints no timing; wall it via second run? keep single-run wall:
        zg_index_s = None  # measured below on re-run is noop; skip
    results["zg_index_note"] = r.stderr[-300:] if r.returncode != 0 else "ok"

    for i, (label, query, pattern, expected, level) in enumerate(QUERIES):
        if SPLIT == "dev" and i not in DEV_IDX["queries"]:
            continue
        if SPLIT == "held" and i in DEV_IDX["queries"]:
            continue
        row = run_keyword_set(label, query, pattern, expected, ws)
        row["level"] = level
        row["split"] = "dev" if i in DEV_IDX["queries"] else "held"
        results["queries"].append(row)
        print(json.dumps(row), flush=True)

    results["concepts"] = []
    for i, (label, query, expected, level) in enumerate(CONCEPTS):
        if SPLIT == "dev" and i not in DEV_IDX["concepts"]:
            continue
        if SPLIT == "held" and i in DEV_IDX["concepts"]:
            continue
        row = run_concept_set(label, query, expected, ws)
        row["level"] = level
        row["split"] = "dev" if i in DEV_IDX["concepts"] else "held"
        results["concepts"].append(row)
        print(json.dumps(row), flush=True)

    def mean(rows, key):
        vs = [q[key] for q in rows if q[key] is not None]
        return round(statistics.mean(vs), 1) if vs else None

    def rate(rows, key):
        vs = [q[key] for q in rows if q[key] is not None]
        return f"{sum(vs)}/{len(vs)}" if vs else None

    def table(title, rows, systems):
        print(f"\n{title} (split={SPLIT})")
        print("| system | mean ms | recall@3 |")
        print("|---|---|---|")
        for name, lat, rec in systems:
            print(f"| {name} | {mean(rows, lat)} | {rate(rows, rec)} |")

    def by_level(rows):
        out = {}
        for q in rows:
            out.setdefault(q.get("level", "?"), []).append(q)
        return out

    for title, rows, systems in [
        ("keyword set", results["queries"], [
            ("rg -l", "rg_ms", "rg_recall3"),
            ("one-grep lexical", "lex_ms", "lex_recall3"),
            ("one-grep hybrid", "hyb_ms", "hyb_recall3"),
            ("one-grep rerank", "rkh_ms", "rkh_recall3"),
            ("zg", "zg_ms", "zg_recall3"),
        ]),
        ("concept set", results["concepts"], [
            ("one-grep lexical", "lex_ms", "lex_recall3"),
            ("one-grep hybrid", "hyb_ms", "hyb_recall3"),
            ("one-grep rerank", "rkh_ms", "rkh_recall3"),
            ("zg", "zg_ms", "zg_recall3"),
        ]),
    ]:
        table(title, rows, systems)
        levels = by_level(rows)
        if len(levels) > 1:
            print(f"by difficulty: " + "; ".join(
                f"L{lv} hybrid {rate(rs, 'hyb_recall3')}"
                for lv, rs in sorted(levels.items(), key=lambda kv: str(kv[0]))))
    print(f"\nindex: one-grep {results['onegrep_index_s']}s, "
          f"embed {results['onegrep_embed_s']}s; zg: {results['zg_index_note']}")
    (tmp / "results.json").write_text(json.dumps(results, indent=1))
    print(f"\nraw: {tmp}/results.json  corpus kept at {ws}")


if __name__ == "__main__":
    sys.exit(main())
