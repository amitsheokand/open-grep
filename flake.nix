{
  description = "one-grep: local-first hybrid workspace search (package + home-manager module)";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
  };

  outputs =
    { self, nixpkgs }:
    let
      systems = [
        "aarch64-darwin"
        "aarch64-linux"
        "x86_64-linux"
      ];
      forAllSystems = nixpkgs.lib.genAttrs systems;
    in
    {
      overlays.default = final: prev: {
        one-grep = final.callPackage ./nix/package.nix { };
      };

      packages = forAllSystems (
        system:
        let
          pkgs = import nixpkgs {
            inherit system;
            overlays = [ self.overlays.default ];
          };
        in
        {
          one-grep = pkgs.one-grep;
          default = pkgs.one-grep;
        }
      );

      # Home Manager module (import into a consumer HM config).
      homeManagerModules.one-grep = import ./nix/home-manager/one-grep.nix;
      homeManagerModules.default = self.homeManagerModules.one-grep;

      # Cheap checks only (no rust compile / cargo fetch).
      # Full `nix build .#one-grep` is validate-on-nixos for Linux hosts.
      checks = forAllSystems (
        system:
        let
          pkgs = nixpkgs.legacyPackages.${system};
        in
        {
          module-file = pkgs.runCommand "one-grep-hm-module-file" { } ''
            test -f ${./nix/home-manager/one-grep.nix}
            test -f ${./nix/package.nix}
            test -f ${./nix/README.md}
            printf 'ok\n' > "$out"
          '';
        }
      );

      formatter = forAllSystems (system: nixpkgs.legacyPackages.${system}.nixfmt);
    };
}
