{
  description = "GDS native desktop applications";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/02c087f3fe673c9e317f206a627cb6b910336dbe";

  outputs = { self, nixpkgs, ... }:
    let
      systems = [ "x86_64-linux" "aarch64-linux" ];
      forEachSystem = nixpkgs.lib.genAttrs systems;
      applications = builtins.attrNames (nixpkgs.lib.filterAttrs
        (_: type: type == "directory") (builtins.readDir ./packaging/nix/apps));
      applicationNames = map (name: "gds-${name}") applications;
      buildTag = builtins.substring 0 9 (self.rev or self.dirtyRev or "unknown");
    in {
      packages = forEachSystem (system:
        let
          pkgs = import nixpkgs { inherit system; };
          packages = nixpkgs.lib.genAttrs applicationNames (name:
            pkgs.callPackage (./packaging/nix/apps + "/${nixpkgs.lib.removePrefix "gds-" name}/package.nix") { inherit buildTag; });
          suite = pkgs.buildEnv {
            name = "gds";
            paths = builtins.attrValues packages;
            meta.description = "GDS native desktop application suite";
          };
        in packages // {
          gds = suite;
          xdg-desktop-portal-gds = pkgs.callPackage ./packaging/nix/portal/package.nix { inherit buildTag; };
          default = suite;
        });

      apps = forEachSystem (system:
        nixpkgs.lib.genAttrs applicationNames (name: {
          type = "app";
          meta.description = self.packages.${system}.${name}.meta.description;
          program = "${self.packages.${system}.${name}}/bin/${name}";
        }));

      nixosModules = nixpkgs.lib.genAttrs applicationNames (name:
        import (./packaging/nix/apps + "/${nixpkgs.lib.removePrefix "gds-" name}/module.nix") self
      ) // {
        gds-portal = import ./packaging/nix/portal/module.nix self;
        default = {
          imports = map (name: self.nixosModules.${name}) applicationNames
            ++ [ self.nixosModules.gds-portal ];
        };
      };

      checks = forEachSystem (system:
        {
          portal = self.packages.${system}.xdg-desktop-portal-gds;
        });

      devShells = forEachSystem (system:
        let
          pkgs = import nixpkgs { inherit system; };
        in {
          default = import ./packaging/nix/dev-shell.nix { inherit pkgs; };
        });
    };
}
