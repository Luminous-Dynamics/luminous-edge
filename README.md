# Luminous Edge

Meta-flake composing the [Luminous Platform](https://github.com/Luminous-Dynamics/luminous-platform) layer for NixOS hosts.

## What it provides

One flake input instead of three:

```nix
inputs.luminous-edge.url = "github:Luminous-Dynamics/luminous-edge";
```

Gives you:
- `nixosModules.sovereignBoot` — boot animation, fail-open state machine, Linux recovery
- `nixosModules.nixward` — conscious NixOS management + machine contracts
- `packages.<system>.sporeBootTools` — pre-built boot binaries
- `packages.<system>.rea` and `apps.<system>.rea` — Nix-managed launcher for the pinned REA reverse-engineering CLI

## Components

| Component | Source |
|-----------|--------|
| [sovereign-boot](https://github.com/Luminous-Dynamics/sovereign-boot) | Boot ecology (quicken-fb, spore-boot-state, spore-recovery-linux) |
| [nixward](https://github.com/Luminous-Dynamics/nixward) | NixOS management, machine contracts, warded node security |
| [REA](https://rea.tools/) | Reverse-engineering workbench for binaries, applications, and runtime behavior |

## NixOS Integration

```nix
# flake.nix
inputs.luminous-edge.url = "github:Luminous-Dynamics/luminous-edge";

# In your NixOS configuration
{ inputs, ... }: {
  imports = [
    inputs.luminous-edge.nixosModules.sovereignBoot
    inputs.luminous-edge.nixosModules.nixward
  ];

  # Enable sovereign boot (disabled by default — QEMU gates must pass first)
  # luminous.services.sporeBoot.enable = true;
}
```

## REA CLI

Run the pinned REA CLI through Nix without changing system or agent configuration:

```sh
nix run github:Luminous-Dynamics/luminous-edge#rea -- --version
nix run github:Luminous-Dynamics/luminous-edge#rea -- doctor --json
```

The launcher pins `rea-agents@6.4.0` and uses Node.js 26.10.0 from the flake's locked nixpkgs. The npm package is downloaded into the user's cache on first invocation; this is a Nix-managed launcher, not a fully offline/store-hermetic npm build. To make it available permanently, add `inputs.luminous-edge.packages.${pkgs.system}.rea` to `environment.systemPackages` or Home Manager's `home.packages`.

Review the install/setup behavior and current release limitations in [REA on Nix](docs/rea-with-nix.md). Installing the launcher does not run `rea setup`; setup can modify agent configuration, so run `rea setup --dry-run` before approving `rea setup`.

## Safety Contract

> Platform components may observe host state; they must never be required for host boot.

`sporeBoot.enable` is `false` by default. Do not enable it on a physical host until QEMU qualification gates pass.

## License

AGPL-3.0-or-later.
