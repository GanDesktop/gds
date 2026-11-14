{ callPackage, cmake, alsa-lib, procps, buildTag ? "unknown" }:
callPackage ../../package.nix {
  inherit buildTag;
  application = "agents";
  description = "Native GPUI workspace for Bas agents and conversations";
  extraNativeBuildInputs = [ cmake ];
  extraBuildInputs = [ alsa-lib ];
  extraNativeCheckInputs = [ procps ];
  desktopItem = {
    name = "io.github.GanDesktop.GdsAgents";
    exec = "gds-agents";
    icon = "io.github.GanDesktop.GdsAgents";
    categories = [ "Development" "Utility" ];
  };
}
