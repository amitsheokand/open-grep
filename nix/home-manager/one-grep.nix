# Home Manager module for one-grep (OSS-safe).
#
# Import from the one-grep flake:
#   imports = [ inputs.one-grep.homeManagerModules.one-grep ];
#   programs.one-grep = {
#     enable = true;
#     package = inputs.one-grep.packages.${pkgs.system}.one-grep;
#     mcp.cursor.enable = true;
#     mcp.opencode.enable = true;
#   };
#
# All config paths are options (defaults match `one-grep install` targets).
# No private hostnames, product names, or user home paths are baked in.
{
  config,
  lib,
  pkgs,
  ...
}:

let
  cfg = config.programs.one-grep;
  inherit (lib)
    mkEnableOption
    mkIf
    mkOption
    types
    ;

  command =
    if cfg.command != null then
      cfg.command
    else if cfg.package != null then
      "${cfg.package}/bin/one-grep"
    else
      "one-grep";

  stdioArgs = [
    "serve"
    "--stdio"
  ];

  cursorLikeEntry = {
    inherit command;
    args = stdioArgs;
  };

  commandCodeEntry = {
    transport = "stdio";
    enabled = true;
    inherit command;
    args = stdioArgs;
  };

  museLegacyEntry = {
    inherit command;
    args = stdioArgs;
    enabled = true;
    mode = "optional";
  };

  opencodeLocalEntry = {
    type = "local";
    command = [ command ] ++ stdioArgs;
    enabled = true;
  };

  abs = rel: "\"$HOME\"/${rel}";

  jqUpsert =
    relPath: mapKey: entry:
    let
      entryJson = builtins.toJSON entry;
    in
    ''
      {
        path=${abs relPath}
        mkdir -p "$(dirname "$path")"
        if [[ -f "$path" ]]; then
          ${pkgs.jq}/bin/jq --argjson entry '${entryJson}' \
            '.${mapKey} = ((.${mapKey} // {})) | .${mapKey}["one-grep"] = $entry' \
            "$path" >"$path.tmp" && mv "$path.tmp" "$path"
        else
          ${pkgs.jq}/bin/jq -n --argjson entry '${entryJson}' \
            '{${mapKey}: {"one-grep": $entry}}' >"$path"
        fi
      }
    '';

  jqUpsertMuse =
    relPath: modern: legacy:
    let
      modernJson = builtins.toJSON modern;
      legacyJson = builtins.toJSON legacy;
    in
    ''
      {
        path=${abs relPath}
        mkdir -p "$(dirname "$path")"
        if [[ -f "$path" ]]; then
          ${pkgs.jq}/bin/jq \
            --argjson modern '${modernJson}' \
            --argjson legacy '${legacyJson}' \
            '.mcpServers = ((.mcpServers // {})) | .mcpServers["one-grep"] = $modern
             | .mcp_servers = ((.mcp_servers // {})) | .mcp_servers["one-grep"] = $legacy' \
            "$path" >"$path.tmp" && mv "$path.tmp" "$path"
        else
          ${pkgs.jq}/bin/jq -n \
            --argjson modern '${modernJson}' \
            --argjson legacy '${legacyJson}' \
            '{mcpServers: {"one-grep": $modern}, mcp_servers: {"one-grep": $legacy}}' >"$path"
        fi
      }
    '';

  hermesUpsertScript = pkgs.writeText "one-grep-hermes-upsert.py" ''
    import pathlib
    import sys

    path = pathlib.Path(sys.argv[1])
    exe = sys.argv[2]
    block = (
        "  one-grep:\n"
        f"    command: {exe}\n"
        "    args:\n"
        "      - serve\n"
        "      - --stdio\n"
    )
    content = path.read_text() if path.exists() else ""
    lines = content.splitlines()
    try:
        mcp_idx = next(i for i, l in enumerate(lines) if l.rstrip() == "mcp_servers:")
    except StopIteration:
        out = content.rstrip()
        if out:
            out += "\n"
        out += "mcp_servers:\n" + block
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(out if out.endswith("\n") else out + "\n")
        raise SystemExit(0)

    end_section = len(lines)
    for i in range(mcp_idx + 1, len(lines)):
        line = lines[i]
        if not line:
            continue
        if not line[0].isspace() and ":" in line:
            end_section = i
            break

    one_start = None
    one_end = None
    for i in range(mcp_idx + 1, end_section):
        line = lines[i]
        if line == "  one-grep:" or line.startswith("  one-grep:"):
            one_start = i
            j = i + 1
            while j < end_section:
                nxt = lines[j]
                if (
                    nxt.startswith("  ")
                    and not nxt.startswith("   ")
                    and not nxt.startswith("  \t")
                    and ":" in nxt
                    and not nxt.startswith("  -")
                ):
                    break
                j += 1
            one_end = j
            break

    block_lines = block.rstrip("\n").splitlines()
    if one_start is not None:
        out_lines = lines[:one_start] + block_lines + lines[one_end:]
    else:
        out_lines = lines[: mcp_idx + 1] + block_lines + lines[mcp_idx + 1 :]
    text = "\n".join(out_lines)
    if content.endswith("\n") and not text.endswith("\n"):
        text += "\n"
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text)
  '';

  activationBody = lib.concatStringsSep "\n" (
    [ "# one-grep MCP registration (idempotent upsert)" ]
    ++ lib.optional cfg.mcp.cursor.enable (
      jqUpsert cfg.mcp.cursor.path "mcpServers" cursorLikeEntry
    )
    ++ lib.optional cfg.mcp.opencode.enable (
      jqUpsert cfg.mcp.opencode.path "mcp" opencodeLocalEntry
    )
    ++ lib.optional cfg.mcp.pi.enable (jqUpsert cfg.mcp.pi.path "mcpServers" cursorLikeEntry)
    ++ lib.optional cfg.mcp.muse.enable (
      jqUpsertMuse cfg.mcp.muse.path cursorLikeEntry museLegacyEntry
    )
    ++ lib.optional cfg.mcp.hermes.enable ''
      {
        path=${abs cfg.mcp.hermes.path}
        ${pkgs.python3}/bin/python3 ${hermesUpsertScript} "$path" ${lib.escapeShellArg command}
      }
    ''
    ++ lib.optional cfg.mcp.commandCode.enable (
      jqUpsert cfg.mcp.commandCode.path "mcpServers" commandCodeEntry
    )
  );

  anyMcp =
    cfg.mcp.cursor.enable
    || cfg.mcp.opencode.enable
    || cfg.mcp.pi.enable
    || cfg.mcp.muse.enable
    || cfg.mcp.hermes.enable
    || cfg.mcp.commandCode.enable;
in
{
  options.programs.one-grep = {
    enable = mkEnableOption "one-grep local-first hybrid workspace search";

    package = mkOption {
      type = types.nullOr types.package;
      default = null;
      example = "pkgs.one-grep";
      description = ''
        one-grep package. Supply via the flake overlay (`overlays.default`) or
        `inputs.one-grep.packages.''${system}.one-grep`. Nullable so hosts can
        wire MCP with only `command` (e.g. a cargo-built binary on PATH).
      '';
    };

    installPackage = mkOption {
      type = types.bool;
      default = true;
      description = ''
        When true and `package` is non-null, add it to `home.packages`.
        Set false if the binary is provided another way (dev symlink, etc.).
      '';
    };

    command = mkOption {
      type = types.nullOr types.str;
      default = null;
      example = "/run/current-system/sw/bin/one-grep";
      description = ''
        Absolute command path written into MCP configs.
        Defaults to `''${package}/bin/one-grep` when package is set, else `"one-grep"` (PATH).
      '';
    };

    mcp = {
      cursor = {
        enable = mkEnableOption "register one-grep in Cursor Agent MCP config";
        path = mkOption {
          type = types.str;
          default = ".cursor/mcp.json";
          description = "Path relative to the home directory.";
        };
      };
      opencode = {
        enable = mkEnableOption "register one-grep in OpenCode MCP config (stdio local)";
        path = mkOption {
          type = types.str;
          default = ".config/opencode/opencode.json";
          description = "Path relative to the home directory.";
        };
      };
      pi = {
        enable = mkEnableOption "register one-grep in Pi agent MCP config";
        path = mkOption {
          type = types.str;
          default = ".pi/agent/mcp.json";
          description = "Path relative to the home directory.";
        };
      };
      muse = {
        enable = mkEnableOption "register one-grep in Muse settings (mcpServers + mcp_servers)";
        path = mkOption {
          type = types.str;
          default = ".config/muse/settings.json";
          description = "Path relative to the home directory.";
        };
      };
      hermes = {
        enable = mkEnableOption "register one-grep in Hermes config.yaml";
        path = mkOption {
          type = types.str;
          default = ".hermes/config.yaml";
          description = "Path relative to the home directory.";
        };
      };
      commandCode = {
        enable = mkEnableOption "register one-grep in Command-Code MCP config";
        path = mkOption {
          type = types.str;
          default = ".commandcode/mcp.json";
          description = "Path relative to the home directory.";
        };
      };
    };
  };

  config = mkIf cfg.enable {
    assertions = [
      {
        assertion = (cfg.package != null) || (cfg.command != null) || (!cfg.installPackage);
        message = ''
          programs.one-grep: set `package` (flake overlay / packages.one-grep)
          or `command`, or set installPackage = false.
        '';
      }
    ];

    home.packages = lib.optional (cfg.installPackage && cfg.package != null) cfg.package;

    home.activation.oneGrepMcp = mkIf anyMcp (
      lib.hm.dag.entryAfter [ "writeBoundary" ] activationBody
    );
  };
}
