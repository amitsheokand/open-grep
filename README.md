# one-grep

Local-first hybrid workspace search: ripgrep + BM25 + ONNX embeddings, with an
in-tree MCP server (`search` + `rg`).

Public repo: [github.com/amitsheokand/open-grep](https://github.com/amitsheokand/open-grep).
The CLI name is still **`one-grep`** (`open-grep` is a Nix wrapper alias).

```sh
one-grep index /path/to/workspace
one-grep embed /path/to/workspace
one-grep query "where is authentication handled?" --path /path/to/workspace --hybrid
one-grep serve --stdio
```

Nix: `nix build github:amitsheokand/open-grep` or import
`homeManagerModules.one-grep`. Details: [INTEGRATION.md](INTEGRATION.md),
[nix/README.md](nix/README.md).
