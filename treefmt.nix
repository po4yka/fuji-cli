{ pkgs, ... }:
{
  projectRootFile = "flake.nix";

  programs = {
    nixfmt = {
      enable = true;
      strict = true;
    };

    rustfmt = {
      enable = true;
      package = pkgs.fujicliToolchain;
    };
    shellcheck.enable = true;
  };

  settings.global.excludes = [ ".envrc" ];
}
