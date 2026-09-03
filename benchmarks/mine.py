#!/usr/bin/env python3
"""Phase A pair mining: synthesize (query, chunk) pairs with the local MLX
lane, attach BM25 hard negatives from one-grep lexical search.

Usage:
  python3 benchmarks/mine.py --workspace WS --out triples.jsonl --n 200

Each line: {query, pos: {path,start,end}, negs: [{path,start,end}...]}.
"""
import argparse
import json
import random
import subprocess
import urllib.request

ONE_GREP = "/Users/amitsheokand/dev/one-grep/target/release/one-grep"
MLX_URL = "http://127.0.0.1:8081/v1/chat/completions"
MLX_MODEL = "/Users/amitsheokand/models/Compactor-Qwen3.5-4B-4bit"

PROMPT = """You write search queries for a code/config search engine.
Given the file chunk below, write ONE natural question (no more than 20 words)
that this chunk answers. Use different words than the chunk where possible.
Reply with only the question, nothing else.

File: {path} [{breadcrumb}]
```
{text}
```"""


def chat(prompt):
    body = json.dumps({
        "model": MLX_MODEL,
        "messages": [{"role": "user", "content": prompt}],
        "temperature": 0.7,
        "max_tokens": 100,
        "stream": False,
    }).encode()
    req = urllib.request.Request(MLX_URL, data=body,
                                 headers={"Content-Type": "application/json"})
    with urllib.request.urlopen(req, timeout=120) as r:
        return json.load(r)["choices"][0]["message"]["content"].strip()


def run(cmd, **kw):
    return subprocess.run(cmd, capture_output=True, text=True, **kw)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--workspace", required=True)
    ap.add_argument("--out", required=True)
    ap.add_argument("--n", type=int, default=200)
    ap.add_argument("--seed", type=int, default=7)
    args = ap.parse_args()
    random.seed(args.seed)

    r = run([ONE_GREP, "dump-chunks", args.workspace])
    assert r.returncode == 0, r.stderr[-500:]
    chunks = [json.loads(l) for l in r.stdout.splitlines() if l.strip()]
    # Negatives need a lexical index; build it if missing.
    import os
    if not os.path.isdir(os.path.join(args.workspace, ".one-grep")):
        r = run([ONE_GREP, "index", args.workspace])
        assert r.returncode == 0, r.stderr[-500:]
    # Skip trivial one-liners; stratify by kind.
    pool = [c for c in chunks
            if len(c["text"].splitlines()) >= 3 and len(c["text"]) >= 120]
    by_kind = {}
    for c in pool:
        by_kind.setdefault(c["kind"], []).append(c)
    per_kind = max(1, args.n // max(1, len(by_kind)))
    sample = []
    for kind, cs in by_kind.items():
        sample += random.sample(cs, min(per_kind, len(cs)))
    random.shuffle(sample)
    sample = sample[:args.n]
    print(f"{len(chunks)} chunks -> {len(sample)} sampled", flush=True)

    kept = 0
    with open(args.out, "w") as f:
        for i, c in enumerate(sample):
            try:
                q = chat(PROMPT.format(
                    path=c["path"], breadcrumb=c["breadcrumb"],
                    text=c["text"][:2000]))
            except Exception as e:  # noqa: BLE001 - keep mining on errors
                print(f"[{i}] chat failed: {e}", flush=True)
                continue
            q = " ".join(q.split())
            if len(q.split()) < 3 or len(q) > 300:
                continue
            # Hard negatives: lexical top-12 excluding the positive file.
            r = run([ONE_GREP, "query", q, "--path", args.workspace,
                     "--limit", "12"])
            negs = []
            if r.returncode == 0:
                for line in r.stdout.splitlines():
                    if not line.startswith("/"):
                        continue
                    head = line.split(" [")[0]
                    if ":" not in head or "-" not in head:
                        continue
                    p, span = head.rsplit(":", 1)
                    s, _, e = span.partition("-")
                    if expected_path(args.workspace, p) == c["path"]:
                        continue
                    try:
                        negs.append({"path": expected_path(args.workspace, p),
                                     "start": int(s), "end": int(e)})
                    except ValueError:
                        continue
                    if len(negs) >= 5:
                        break
            f.write(json.dumps({
                "query": q,
                "pos": {"path": c["path"], "start": c["start"], "end": c["end"]},
                "negs": negs,
            }) + "\n")
            kept += 1
            if kept % 25 == 0:
                print(f"[{i}] kept {kept}", flush=True)
    print(f"done: {kept} triples -> {args.out}")


def expected_path(ws, abspath):
    return abspath.replace(ws.rstrip("/") + "/", "", 1)


if __name__ == "__main__":
    main()
