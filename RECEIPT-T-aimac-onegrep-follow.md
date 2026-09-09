# Receipt: T-aimac-onegrep-follow

## Status: GREEN (COMPLETE)

**Target**: `ai-mac` (macOS arm64)  
**Repo**: `~/dev/one-grep`  
**Commit message**: `onegrep: expand install targets for harnesses`  
**No push.**

---

## What changed

Expanded `one-grep install --target` beyond `opencode` to match already-wired harnesses.

| Target | Config path | Notes |
| :--- | :--- | :--- |
| `opencode` | `~/.config/opencode/opencode.json` | stdio local command **or** `--http` remote+bearer (unchanged) |
| `cursor` | `~/.cursor/mcp.json` | `mcpServers.one-grep` command+args |
| `pi` | `~/.pi/agent/mcp.json` | `mcpServers.one-grep` command+args |
| `muse` | `~/.config/muse/settings.json` | upserts **both** `mcpServers` and `mcp_servers` |
| `hermes` | `~/.hermes/config.yaml` | surgical YAML upsert (preserves anchors/comments) |
| `command-code` | `~/.commandcode/mcp.json` | aliases: `commandcode`, `command_code` |

Binary resolution: prefer `~/.local/bin/one-grep` when present, else `current_exe()`.

---

## Files

- `src/install.rs` — multi-target install + hermes YAML upsert helpers + unit tests
- `src/main.rs` — CLI help + dispatch via `install::install`
- `INTEGRATION.md` — install CLI row + §2.1 install snippet
- `RECEIPT-T-aimac-onegrep-follow.md` — this receipt

---

## Verification

- `cargo test` → **40 passed** (lib) + bin/doc empty suites OK; exit 0
- `cargo check` → exit 0
- `cargo build --release` → exit 0 (strip warning only: missing libLLVM for rust-objcopy; binary produced)
- Smoke: `one-grep install --target` for `opencode`, `cursor`, `pi`, `muse`, `hermes`, `command-code`, `commandcode` — all registered
- Negative: unknown target / `--http` on non-opencode → error as expected
- Harness verify:
  - `hermes mcp test one-grep` → Connected, 2 tools
  - `cursor-agent mcp list` → `one-grep: ready`
  - `command-code mcp list` → `one-grep` enabled
- Hermes YAML anchors (`&id001`) and `zvec_grep` peer entry preserved

---

## Still Queued (out of scope)

1. **NixOS / Vaayu home-manager module** (OSS-safe) for duduk/nix hosts — declarative packaging of harness MCP entries.
2. Embedding/model/China policy changes — not touched.
3. Advait farm Hard parks — not touched.
