#!/bin/zsh
# Phase B: fine-tune MiniLM on mined triples, export ONNX.
~/.local/share/onegrep-ft/venv/bin/python benchmarks/train_ft.py "$@"
