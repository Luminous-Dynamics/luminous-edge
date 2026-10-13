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

## Components

| Component | Source |
|-----------|--------|
| [sovereign-boot](https://github.com/Luminous-Dynamics/sovereign-boot) | Boot ecology (quicken-fb, spore-boot-state, spore-recovery-linux) |
| [nixward](https://github.com/Luminous-Dynamics/nixward) | NixOS management, machine contracts, warded node security |

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

## Research

[REA-RS: a Rust-native successor to REA](docs/rea-rust-successor.md) documents the staged migration and the current read-only Rust artifact-inventory prototype under `rust/rea-rs`. The branch also contains a bounded harness to examine the exact published REA package, its CLI/MCP contract, and its self-analysis attempt. **Neither the Rust code nor self-analysis has passed CI yet**; the Nix-managed REA launcher remains separate in [PR #3](https://github.com/Luminous-Dynamics/luminous-edge/pull/3).

## Safety Contract

> Platform components may observe host state; they must never be required for host boot.

`sporeBoot.enable` is `false` by default. Do not enable it on a physical host until QEMU qualification gates pass.

## License

AGPL-3.0-or-later.
