"""Phase B: fine-tune all-MiniLM-L6-v2 on mined triples (MNRL), export ONNX.

Usage: train.sh --triples /tmp/mined200.jsonl --workspace /tmp/mine-pilot
       --out benchmarks/models/minilm-nixft
"""
import argparse
import json
import random
import subprocess

from sentence_transformers import InputExample, SentenceTransformer
from sentence_transformers.sentence_transformer.losses import (
    MultipleNegativesRankingLoss,
)
from torch.utils.data import DataLoader


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--triples", required=True, nargs="+",
                    help="one or more triples jsonl files (concatenated)")
    ap.add_argument("--workspace", required=True)
    ap.add_argument("--out", required=True)
    ap.add_argument("--epochs", type=int, default=10)
    ap.add_argument("--seed", type=int, default=7)
    args = ap.parse_args()
    random.seed(args.seed)

    out = subprocess.run(
        ["/Users/amitsheokand/dev/one-grep/target/release/one-grep",
         "dump-chunks", args.workspace],
        capture_output=True, text=True)
    assert out.returncode == 0, out.stderr[-500:]
    texts = {}
    for line in out.stdout.splitlines():
        if line.strip():
            c = json.loads(line)
            texts[(c["path"], c["start"], c["end"])] = c["text"][:1500]

    rows = []
    for t in args.triples:
        rows += [json.loads(l) for l in open(t) if l.strip()]
    random.shuffle(rows)
    cut = int(len(rows) * 0.8)
    train_rows, held_rows = rows[:cut], rows[cut:]
    json.dump([r["query"] for r in held_rows],
              open(args.out + ".held_queries.json", "w"))
    json.dump(held_rows, open(args.out + ".held.json", "w"))

    def to_example(r):
        # Self-contained triples carry text; else join via workspace dump.
        if "pos_text" in r:
            return (r["query"], r["pos_text"][:1500])
        pos = texts.get((r["pos"]["path"], r["pos"]["start"], r["pos"]["end"]))
        return (r["query"], pos) if pos else None

    train_pairs = [p for p in (to_example(r) for r in train_rows) if p]
    print(f"train pairs: {len(train_pairs)}, held: {len(held_rows)}", flush=True)
    examples = [InputExample(texts=[q, p]) for q, p in train_pairs]

    model = SentenceTransformer("sentence-transformers/all-MiniLM-L6-v2")
    loader = DataLoader(examples, batch_size=16, shuffle=True)
    loss = MultipleNegativesRankingLoss(model)
    model.fit([(loader, loss)], epochs=args.epochs, warmup_steps=int(len(loader)),
              output_path=args.out, show_progress_bar=False)
    print("saved", args.out, flush=True)


if __name__ == "__main__":
    main()
