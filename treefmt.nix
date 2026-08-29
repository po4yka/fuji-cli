{ pkgs, ... }:
{
  projectRootFile = "flake.nix";

  programs = {
    cue.enable = true;

    nixfmt = {
      enable = true;
      strict = true;
    };

    rustfmt = {
      enable = true;
      package = pkgs.fujicliToolchain;
    };
    shellcheck.enable = true;
    taplo.enable = true;
  };

  settings.global.excludes = [ ".envrc" ];
}
