flake:
let
  homes = flake.homeConfigurations or { };
  systems = flake.nixosConfigurations or { };
in
(map (name: {
  inherit name;
  host = null;
}) (builtins.attrNames homes))
++ (builtins.concatMap (
  host:
  map (name: { inherit name host; }) (
    builtins.attrNames (systems.${host}.config.home-manager.users or { })
  )
) (builtins.attrNames systems))
