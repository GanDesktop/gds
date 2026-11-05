{ system, name }:
let
  # Capture options inside the same module evaluation as the selected user.
  # getSubOptions describes an unconfigured user and cannot resolve config-dependent types.
  inspected = system.extendModules {
    modules = [
      {
        home-manager.sharedModules = [
          ({ options, lib, ... }: {
            options._gdsNixesOptions = lib.mkOption {
              type = lib.types.raw;
              internal = true;
              readOnly = true;
            };
            config._gdsNixesOptions = options;
          })
        ];
      }
    ];
  };
  user = inspected.config.home-manager.users.${name};
in
{
  options = user._gdsNixesOptions;
  config = user;
}
