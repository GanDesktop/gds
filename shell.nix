# Evaluate local Nix definitions directly, without archiving the project or target/.
let
  lock = builtins.fromJSON (builtins.readFile ./flake.lock);
  locked = lock.nodes.nixpkgs.locked;
  source = builtins.fetchTree {
    inherit (locked) type owner repo rev narHash;
  };
  pkgs = import source.outPath { };
in
import ./packaging/nix/dev-shell.nix { inherit pkgs; }
