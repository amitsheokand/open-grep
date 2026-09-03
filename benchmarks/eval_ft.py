"""Held-out eval: pure-vector recall against a merged multi-corpus pool.

Usage: eval_ft.py --model <st-id-or-path> --held held.json --ws WS [WS...]
"""
import argparse
import json
import random
import subprocess

random.seed(7)

# Cap negatives per corpus: huge corpora (windows-rs 600k chunks) would
# take hours to encode. Positives always included.
POOL_CAP = 8000

ONE_GREP = "/Users/amitsheokand/dev/one-grep/target/release/one-grep"


def dump_chunks(ws):
    out = subprocess.run([ONE_GREP, "dump-chunks", ws],
                         capture_output=True, text=True)
    assert out.returncode == 0, out.stderr[-500:]
    return [json.loads(l) for l in out.stdout.splitlines() if l.strip()]


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--model", required=True)
    ap.add_argument("--held", required=True)
    ap.add_argument("--ws", nargs="+", required=True)
    args = ap.parse_args()

    from sentence_transformers import SentenceTransformer, util
    model = SentenceTransformer(args.model)
    held = json.load(open(args.held))
    want_texts = {h.get("pos_text", "")[:1500] for h in held if h.get("pos_text")}
    want_paths = {(h["pos"]["path"], h["pos"]["start"]) for h in held}
    chunks = []
    for ws in args.ws:
        got = dump_chunks(ws)
        # Always include positives, then fill to cap with negatives.
        pos = [c for c in got
               if c["text"][:1500] in want_texts
               or (c["path"], c["start"]) in want_paths]
        neg = [c for c in got if c not in pos]
        if len(neg) > POOL_CAP:
            neg = random.sample(neg, POOL_CAP)
        chunks += pos + neg
    print(f"pool: {len(chunks)} chunks (capped)", flush=True)
    ce = model.encode([c["text"][:1500] for c in chunks],
                      normalize_embeddings=True, show_progress_bar=False)
    r1 = r3 = r10 = scored = 0
    for h in held:
        pos = h.get("pos_text", "")[:1500]
        q = model.encode(h["query"], normalize_embeddings=True)
        scores = util.cos_sim(q, ce)[0]
        order = sorted(range(len(chunks)), key=lambda i: -scores[i])[:10]
        hit_idx = next(
            (i for i, c in enumerate(chunks)
             if pos and c["text"][:1500] == pos), None)
        if hit_idx is None:
            hit_idx = next(
                (i for i, c in enumerate(chunks)
                 if (c["path"], c["start"]) == (h["pos"]["path"], h["pos"]["start"])),
                None)
        if hit_idx is None:
            continue
        scored += 1
        rank = order.index(hit_idx) if hit_idx in order else 99
        if rank == 0:
            r1 += 1
        if rank < 3:
            r3 += 1
        if rank < 10:
            r10 += 1
    print(f"{args.model}: R@1 {r1/scored:.2f} R@3 {r3/scored:.2f} "
          f"R@10 {r10/scored:.2f} (scored {scored}/{len(held)})")


if __name__ == "__main__":
    main()
