# Luminous Edge — meta-flake for the Luminous Platform Layer
#
# Composes sovereign-boot and nixward into one host-facing input.
#
# Qualification is intentionally fail-closed: the renderer is available as a
# useful component, but the edge facade is not considered fully qualified until
# sovereign-boot exports both lifecycle executables required by its module.
{
  description = "Luminous Edge — NixOS platform layer (sovereign-boot + nixward)";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";

    sovereign-boot = {
      url = "github:Luminous-Dynamics/sovereign-boot";
    };

    nixward = {
      url = "github:Luminous-Dynamics/nixward";
    };
  };

  outputs = { self, nixpkgs, flake-utils, sovereign-boot, nixward }:
    let
      sovereignBootModule = sovereign-boot.nixosModules.sovereignBoot;
      nixwardModule =
        if builtins.hasAttr "default" nixward.nixosModules
        then nixward.nixosModules.default
        else nixward.nixosModules.nixward;

    in
    {
      nixosModules = {
        sovereignBoot = sovereignBootModule;
        sporeBoot = sovereignBootModule;
        nixward = nixwardModule;
      };

      nixosModules.default = { ... }: {
        imports = [
          sovereignBootModule
          nixwardModule
        ];
      };

    } // flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs { inherit system; };
        sovereignBootPackages = sovereign-boot.packages.${system};
        sporeBootRenderer = sovereignBootPackages.quicken-fb;
        requiredLifecycleTools = [
          "spore-boot-state"
          "spore-recovery-linux"
        ];
        missingLifecycleTools =
          builtins.filter
            (name: !builtins.hasAttr name sovereignBootPackages)
            requiredLifecycleTools;

        extractionCompletenessCheck =
          pkgs.runCommand "luminous-edge-sovereign-boot-extraction-completeness" {} ''
            echo "Required lifecycle tools: ${builtins.concatStringsSep ", " requiredLifecycleTools}"
            echo "Missing lifecycle tools: ${builtins.concatStringsSep ", " missingLifecycleTools}"
            if [ -n "${builtins.concatStringsSep " " missingLifecycleTools}" ]; then
              echo "FAIL: sovereign-boot extraction is incomplete."
              echo "The edge facade must not claim qualification without both lifecycle executables."
              exit 1
            fi
            touch "$out"
          '';
      in
      {
        packages = {
          # Honest compatibility name retained; this is the renderer package,
          # not the lifecycle state/recovery toolset.
          sporeBootTools = sporeBootRenderer;
          default = sporeBootRenderer;
        };

        checks.sovereign-boot-extraction-completeness = extractionCompletenessCheck;
        checks.spore-boot-renderer = sporeBootRenderer;

        checks.module-export-shape = pkgs.runCommand "luminous-edge-module-export-shape" {} ''
          test "${if builtins.isFunction sovereignBootModule then "yes" else "no"}" = yes
          test "${if builtins.isFunction nixwardModule then "yes" else "no"}" = yes
          touch "$out"
        '';

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
