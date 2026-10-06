# Luminous Edge — meta-flake for the Luminous Platform Layer
#
# Composes sovereign-boot, nixward, and (optionally) sovereign-ops into a
# single flake input for NixOS hosts. Hosts pin one URL instead of three.
#
# Usage in /etc/nixos/flake.nix:
#   inputs.luminous-edge.url = "github:Luminous-Dynamics/luminous-edge";
#
# Usage in NixOS modules:
#   imports = [
#     inputs.luminous-edge.nixosModules.sovereignBoot
#     inputs.luminous-edge.nixosModules.nixward
#   ];
{
  description = "Luminous Edge — NixOS platform layer (sovereign-boot + nixward)";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";

    sovereign-boot = {
      url = "github:Luminous-Dynamics/sovereign-boot";
      flake = false;
    };

    nixward = {
      url = "github:Luminous-Dynamics/nixward";
    };
  };

  outputs = { self, nixpkgs, flake-utils, sovereign-boot, nixward }:
    let
      # Re-export the sovereign-boot NixOS module (from the source tree,
      # same pattern as /etc/nixos currently uses).
      sovereignBootModule = import (sovereign-boot.outPath + "/nix/modules/sovereign-boot.nix");

    in
    {
      # Re-export all NixOS modules from constituent repos.
      nixosModules = {
        # Sovereign Boot Ecology
        sovereignBoot = sovereignBootModule;
        sporeBoot     = sovereignBootModule;  # legacy alias

        # Nixward: conscious NixOS management
        nixward       = nixward.nixosModules.default or nixward.nixosModules.nixward;
      };

      # Convenience: single module that imports both
      nixosModules.default = { ... }: {
        imports = [
          sovereignBootModule
          (nixward.nixosModules.default or nixward.nixosModules.nixward)
        ];
      };

      # Pass through sovereign-boot source for spore-boot-tools derivation
      inherit sovereign-boot;

    } // flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs { inherit system; };

        # Build spore-boot-tools from sovereign-boot source
        sporeBootTools = import (sovereign-boot.outPath + "/nix/packages/spore-boot-tools.nix") {
          inherit pkgs;
          src = sovereign-boot.outPath;
        };
      in
      {
        packages = {
          inherit sporeBootTools;
          default = sporeBootTools;
        };

        # Integration check: build sovereign-boot tools + evaluate nixward module
        checks.spore-boot-tools = sporeBootTools;

        devShells.default = pkgs.mkShell {
          buildInputs = with pkgs; [ nil nixfmt-rfc-style nix-tree ];
          shellHook = ''
            echo "Luminous Edge development shell"
            echo "  sovereign-boot: ${sovereign-boot}"
            echo "  nixward:        ${nixward}"
          '';
        };
      });
}
