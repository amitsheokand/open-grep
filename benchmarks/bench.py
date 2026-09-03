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

ONE_GREP = Path("/Users/amitsheokand/dev/one-grep/target/release/one-grep")
if not ONE_GREP.exists():
    ONE_GREP = Path("/Users/amitsheokand/dev/one-grep/target/debug/one-grep")
CORPUS_SRC = Path("/Users/amitsheokand/dev/nixos-config")

QUERIES = [
    # (label, one-grep/zg query, rg pattern, expected path substring)
    ("mlx server", "mlx language model server", "mlx-lm-server", "mlx-mac.nix"),
    ("rust setup", "rust toolchain installation setup", "rustup", "home-manager.nix"),
    ("launchd svc", "background service launchd agent", "launchd", "hosts/darwin/default.nix"),
    ("compactor", "compactor model adapter fusion", "compactor", "mlx-compactor.nix"),
    ("tmux keys", "tmux key bindings pane navigation", "select-pane", "home-manager.nix"),
    ("font cfg", "terminal font family size", "MesloLGS", "home-manager.nix"),
    ("ssh hosts", "ssh host configuration", "matchBlocks", "home-manager.nix"),
    ("stop gemma", "stop gemma lane and start compact lane", "stop gemma",
     "mlx-lane.nix"),
]

# Paraphrased, low keyword overlap with targets.
CONCEPTS = [
    ("never both", "exclusive lane, never both at once", "mlx-lane.nix"),
    ("pretty prompt", "make the terminal prompt show git status with pretty colors",
     "home-manager.nix"),
    ("save session", "automatically preserve my terminal session every few minutes",
     "home-manager.nix"),
    ("no secrets", "where do I put tokens so they never get committed",
     "home-manager.nix"),
]


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

    for label, query, pattern, expected in QUERIES:
        row = run_keyword_set(label, query, pattern, expected, ws)
        results["queries"].append(row)
        print(json.dumps(row), flush=True)

    results["concepts"] = []
    for label, query, expected in CONCEPTS:
        row = run_concept_set(label, query, expected, ws)
        results["concepts"].append(row)
        print(json.dumps(row), flush=True)

    def mean(rows, key):
        vs = [q[key] for q in rows if q[key] is not None]
        return round(statistics.mean(vs), 1) if vs else None

    def rate(rows, key):
        vs = [q[key] for q in rows if q[key] is not None]
        return f"{sum(vs)}/{len(vs)}" if vs else None

    def table(title, rows, systems):
        print(f"\n{title}")
        print("| system | mean ms | recall@3 |")
        print("|---|---|---|")
        for name, lat, rec in systems:
            print(f"| {name} | {mean(rows, lat)} | {rate(rows, rec)} |")

    table("keyword set", results["queries"], [
        ("rg -l", "rg_ms", "rg_recall3"),
        ("one-grep lexical", "lex_ms", "lex_recall3"),
        ("one-grep hybrid", "hyb_ms", "hyb_recall3"),
        ("one-grep rerank", "rkh_ms", "rkh_recall3"),
        ("zg", "zg_ms", "zg_recall3"),
    ])
    table("concept set", results["concepts"], [
        ("one-grep lexical", "lex_ms", "lex_recall3"),
        ("one-grep hybrid", "hyb_ms", "hyb_recall3"),
        ("one-grep rerank", "rkh_ms", "rkh_recall3"),
        ("zg", "zg_ms", "zg_recall3"),
    ])
    print(f"\nindex: one-grep {results['onegrep_index_s']}s, "
          f"embed {results['onegrep_embed_s']}s; zg: {results['zg_index_note']}")
    (tmp / "results.json").write_text(json.dumps(results, indent=1))
    print(f"\nraw: {tmp}/results.json  corpus kept at {ws}")


if __name__ == "__main__":
    sys.exit(main())
