{
  lib,
  rustPlatform,
  makeBinaryWrapper,
  pkg-config,
  helix,
  clippy,
  rustfmt,
  installShellFiles,
}:

let
  cargoToml = fromTOML (builtins.readFile ../../rust/Cargo.toml);
in
rustPlatform.buildRustPackage (finalAttrs: {
  pname = cargoToml.package.name;
  inherit (cargoToml.package) version;

  src = lib.sourceFilesBySuffices ../../rust [
    ".rs"
    ".toml"
    ".lock"
    ".nix"
  ];

  cargoLock = {
    lockFile = ../../rust/Cargo.lock;
  };

  nativeBuildInputs = [
    makeBinaryWrapper
    pkg-config
    installShellFiles
  ];

  buildInputs = [
  ];

  passthru.tests = {
    lint-format = finalAttrs.finalPackage.overrideAttrs (
      _: previousAttrs: {
        pname = previousAttrs.pname + "-lint-format";
        nativeCheckInputs = (previousAttrs.nativeCheckInputs or [ ]) ++ [
          clippy
          rustfmt
        ];
        checkPhase = ''
          cargo clippy
          cargo fmt --check
        '';
      }
    );
  };

  postInstall = ''
    wrapProgram $out/bin/ne --prefix PATH : ${
      lib.makeBinPath [
        helix
      ]
    }
  '';

  stripAllList = [ "bin" ];

  meta = with lib; {
    homepage = "https://github.com/solideva/kopfrot";
    license = licenses.gpl3;
    maintainers = with lib.maintainers; [
      solideva
      sourpup
    ];
    mainProgram = "";
  };
})
