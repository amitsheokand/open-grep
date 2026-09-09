# one-grep Nix packaging (OSS)

Home-manager module and flake overlay for declarative install of `one-grep` and
optional MCP registration for supported agent harnesses.

**Privacy / OSS rules:** this tree must stay free of private product names,
internal hostnames, and user-specific absolute paths. Config paths are module
options with the same relative defaults as `one-grep install`.

## Flake outputs

| Output | Purpose |
| :--- | :--- |
| `packages.<system>.one-grep` | `rustPlatform` build of the CLI/MCP binary |
| `overlays.default` | exposes `pkgs.one-grep` |
| `homeManagerModules.one-grep` | Home Manager module (`programs.one-grep`) |

## Home Manager options

```nix
programs.one-grep = {
  enable = true;

  # From this flake's overlay or packages:
  package = pkgs.one-grep;
  # installPackage = true;   # default: add to home.packages
  # command = null;          # default: "${package}/bin/one-grep"

  mcp = {
    cursor.enable = true;       # ~/.cursor/mcp.json
    opencode.enable = true;     # ~/.config/opencode/opencode.json (stdio)
    pi.enable = true;           # ~/.pi/agent/mcp.json
    muse.enable = true;         # ~/.config/muse/settings.json
    hermes.enable = true;       # ~/.hermes/config.yaml
    commandCode.enable = true;  # ~/.commandcode/mcp.json

    # Override relative paths if a host uses non-default locations:
    # cursor.path = ".cursor/mcp.json";
  };
};
```

### Example consumer snippet

```nix
# In a home-manager config flake:
{
  inputs.one-grep.url = "path:/path/to/one-grep"; # or git URL when published
  # ...
  home-manager.users.alice = { pkgs, ... }: {
    imports = [ inputs.one-grep.homeManagerModules.one-grep ];
    nixpkgs.overlays = [ inputs.one-grep.overlays.default ];

    programs.one-grep = {
      enable = true;
      package = pkgs.one-grep;
      mcp = {
        cursor.enable = true;
        opencode.enable = true;
        pi.enable = true;
      };
    };
  };
}
```

MCP activation **upserts** only the `one-grep` entry (JSON via `jq`, Hermes YAML
via a small Python helper) so peer servers in the same file are preserved.

OpenCode registration is **stdio local** (same as `one-grep install --target opencode`).
HTTP/bearer mode is intentionally out of scope for this module — use the CLI
`install --http` if needed.

## Package build notes

`nix/package.nix` sets `ORT_STRATEGY=system` and links `onnxruntime` from
nixpkgs so the build does not download ORT binaries inside the sandbox.

Validate:

```bash
nix flake check          # eval-only checks (package drv + module file)
nix build .#one-grep     # full compile (may be heavy on Darwin)
```

If a full Darwin build is impractical, treat `nix build` as
**validate-on-nixos** for Linux hosts that consume this module.

## Layout

```
flake.nix
nix/
  package.nix
  README.md
  home-manager/
    one-grep.nix
```
