{ pkgs, version ? "6.4.0" }:

assert builtins.match "[0-9]+\\.[0-9]+\\.[0-9]+" version != null;

pkgs.writeShellApplication {
  name = "rea";

  # REA currently ships through npm rather than as a native Nix package.
  # Nixpkgs pinned by this flake provides Node.js 26.10.0, satisfying REA's
  # current >=26 engine constraint without relying on a possibly older 24.x.
  # The npm package and its JS dependencies are fetched into the user's cache
  # on first invocation, not vendored into the Nix store.
  runtimeInputs = [ pkgs.nodejs_26 ];

  text = ''
    export npm_config_cache="''${XDG_CACHE_HOME:-$HOME/.cache}/rea/npm"
    exec npm exec --yes --package=rea-agents@${version} -- rea "$@"
  '';

  meta = with pkgs.lib; {
    description = "Nix-managed launcher for the pinned REA reverse-engineering CLI";
    homepage = "https://rea.tools/";
    license = licenses.mit;
    mainProgram = "rea";
    platforms = platforms.unix;
  };
}
