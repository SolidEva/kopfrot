{
  system ? builtins.currentSystem,
}:
let
  sources = import ./lon.nix;
  pkgs = import sources.nixpkgs { inherit system; };
  lib = pkgs.lib;
in
{
  packages = lib.recurseIntoAttrs (import ./nix/packages { inherit pkgs; });

  checks = lib.recurseIntoAttrs {
    pre-commit = import ./nix/pre-commit.nix;
  };
}
