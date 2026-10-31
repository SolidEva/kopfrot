let
  sources = import ./lon.nix;
  pkgs = import sources.nixpkgs { };
in
pkgs.mkShell {
  packages = [
    pkgs.nixfmt
    pkgs.clippy
    pkgs.cargo
    pkgs.rustc
    pkgs.rustfmt
    pkgs.rust-analyzer
  ];

  inputsFrom = [
    (import ./default.nix { }).packages.kopfrot
  ];

  shellHook = ''
    ${(import ./nix/pre-commit.nix).shellHook}
  '';

  RUST_SRC_PATH = "${pkgs.rust.packages.stable.rustPlatform.rustLibSrc}";
}
