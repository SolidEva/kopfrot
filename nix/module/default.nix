{
  config,
  lib,
  pkgs,
  ...
}:

let
  cfg = config.programs.kopfrot;
in
{
  options = {
    programs.kopfrot = {
      enable = lib.mkOption {
        default = false;
        type = lib.types.bool;
        description = ''
          Enables kopfrot
        '';
      };
      settings = lib.mkOption {
        default = { };
        type = lib.types.attrs;
        example = {
          vault_directory = /home/user/notes;
          template_directory = /home/user/notes/.templates;
        };
      };
      package = lib.mkOption {
        default = pkgs.callPackage ../packages/kopfrot.nix { };
        defaultText = lib.literalExpression "${pkgs.callPackage ../packages/kopfrot.nix { }}";
        description = "The hello package to use.";
        type = lib.types.package;
      };
    };
  };

  config = lib.mkIf cfg.enable {
    environment.etc."xdg/kopfrot/config.toml".text = lib.mkIf (cfg.settings != { }) (
      lib.concatStrings (
        lib.mapAttrsToList (n: v: ''
          ${n}="${toString v}"
        '') cfg.settings
      )
    );

    environment.systemPackages = [
      cfg.package
    ];
  };
}
