{
  outputs = { self }:
  let
    lock = builtins.fromJSON @nixpkgs-lock@;
    source = builtins.fetchTree {
      inherit (lock.nodes.nixpkgs.locked) type owner repo rev narHash;
    };
    lib = import (source.outPath + "/lib");
    userModule = { config, pkgs, name, hostLabel, ... }: {
      options = {
        home.stateVersion = lib.mkOption { type = lib.types.str; };
        home.username = lib.mkOption { type = lib.types.str; default = name; };
        # Reading this definition needs the configured user's stateVersion via pkgs.
        programs.editor = if pkgs.version != "" then {
          enable = lib.mkOption { type = lib.types.bool; default = true; };
          hostContext = lib.mkOption { type = lib.types.str; default = hostLabel; };
          version = lib.mkOption { type = lib.types.str; default = pkgs.version; };
        } else {};
      };
      config._module.args.pkgs.version = config.home.stateVersion;
    };
    customModule = { lib, ... }: {
      options.customFeature.enable = lib.mkEnableOption "custom feature";
      options.customFeature.customFlag = lib.mkOption { type = lib.types.bool; default = false; };
      config.customFeature = { enable = true; customFlag = true; };
    };
  in {
    nixosConfigurations.workstation = lib.evalModules {
      modules = [({ config, ... }: {
        options.home-manager = {
          sharedModules = lib.mkOption { type = lib.types.listOf lib.types.deferredModule; default = []; };
          users = lib.mkOption {
            type = lib.types.attrsOf (lib.types.submoduleWith {
              specialArgs.hostLabel = "workstation";
              modules = [userModule] ++ config.home-manager.sharedModules;
            });
          };
        };
        config.home-manager.users = {
          alice = { imports = [customModule]; home.stateVersion = "25.11"; };
          bob = { imports = [customModule]; home.stateVersion = "26.05"; };
        };
      })];
    };
  };
}
