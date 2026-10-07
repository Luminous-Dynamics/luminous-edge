# Luminous Edge

Meta-flake composing the [Luminous Platform](https://github.com/Luminous-Dynamics/luminous-platform) layer for NixOS hosts.

## What it provides

One flake input instead of multiple platform component inputs:

```nix
inputs.luminous-edge.url = "github:Luminous-Dynamics/luminous-edge";
```

Gives you:
- `nixosModules.sovereignBoot` — boot animation and the exported Sovereign Boot module; state/recovery enablement remains qualification-gated
- `nixosModules.nixward` — conscious NixOS management + machine contracts
- `packages.<system>.sporeBootTools` — pre-built renderer package

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

## Safety Contract

> Platform components may observe host state; they must never be required for host boot.
>
> **Qualification rule:** the edge facade is not considered qualified unless every transitive component has an independently buildable, pinned, tested head.

`sporeBoot.enable` is `false` by default. Do not enable it on a physical host until QEMU qualification gates pass.

## License

AGPL-3.0-or-later. See [LICENSE](LICENSE). Commercial licensing is described in [COMMERCIAL_LICENSE.md](COMMERCIAL_LICENSE.md).
