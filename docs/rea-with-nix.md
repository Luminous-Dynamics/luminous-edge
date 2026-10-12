# REA on Nix

Luminous Edge exposes a Nix-managed launcher for [REA: Reverse Engineer Anything](https://rea.tools/).

## Quick start

Run the pinned REA CLI without changing the system configuration:

```sh
nix run github:Luminous-Dynamics/luminous-edge#rea -- --version
nix run github:Luminous-Dynamics/luminous-edge#rea -- doctor --json
```

The flake currently pins the upstream npm package version to `rea-agents@6.4.0`. The Node.js 24 runtime comes from the locked `nixpkgs` input. The first invocation needs network access: the launcher runs the pinned npm package and caches it under `$XDG_CACHE_HOME/rea/npm` (or `~/.cache/rea/npm` when XDG_CACHE_HOME is unset).

**Scope of reproducibility:** the launcher and Node.js runtime are Nix-managed, and the application version is pinned. REA's npm package and JavaScript dependency closure are still fetched by npm at runtime; they are not built into the Nix store by this wrapper. This is not a fully offline or fully store-hermetic package.

## Make the command available in NixOS

With `inputs.luminous-edge.url = "github:Luminous-Dynamics/luminous-edge";` in your system flake, add the package to the relevant NixOS module:

```nix
{ inputs, pkgs, ... }: {
  environment.systemPackages = [
    inputs.luminous-edge.packages.${pkgs.system}.rea
  ];
}
```

For Home Manager, use the same package in `home.packages`:

```nix
{ inputs, pkgs, ... }: {
  home.packages = [
    inputs.luminous-edge.packages.${pkgs.system}.rea
  ];
}
```

Rebuild/apply the configuration using your normal NixOS or Home Manager workflow. The launcher is also available as `nix run github:Luminous-Dynamics/luminous-edge#rea -- <args>`.

## Connect an agent only after reviewing the plan

Start with the read-only health check:

```sh
rea doctor --json
```

Then inspect the proposed configuration changes without applying them:

```sh
rea setup --dry-run
```

When ready, run `rea setup` interactively, review the exact plan, and approve only the agent registrations and optional tools you actually want. REA setup can edit local agent configuration; installing the Nix launcher does not run setup or modify agent configuration by itself.

## Current release boundary

The launcher is pinned to `rea-agents@6.4.0`, the latest published package verified during this change. Upstream `main` documents a WASM inspection command, but that command is not advertised by the 6.4.0 package checked for this integration. Do not assume `inspect-wasm-artifact` is available through this Nix launcher until a published REA release includes it or a separately pinned source build has been qualified.

Deep native binary analysis still requires a supported provider such as Ghidra, Hopper, or IDA. The launcher does not install these automatically.
