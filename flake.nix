{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

    treefmt-nix = {
      url = "github:numtide/treefmt-nix";
      inputs.nixpkgs.follows = "nixpkgs";
    };

    fenix = {
      url = "github:nix-community/fenix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    inputs:
    {
      overlays.default =
        final: prev:
        let
          pkgs = final;
        in
        {
          fujicli = pkgs.rustPlatform.buildRustPackage {
            pname = "fujicli";
            version = "0.2.0";

            src = ./.;
            cargoLock.lockFile = ./Cargo.lock;

            nativeBuildInputs = with pkgs; [
              cue
              pkg-config
            ];

            buildInputs = with pkgs; [ libusb1 ];
          };
        };
    }
    // (
      let
        system = "x86_64-linux";

        pkgs = import inputs.nixpkgs {
          inherit system;
          overlays = [
            inputs.self.overlays.default
          ];
        };

        treefmt = inputs.treefmt-nix.lib.evalModule pkgs ./treefmt.nix;
        toolchain = inputs.fenix.packages.${system}.fromToolchainFile {
          file = ./rust-toolchain.toml;
          sha256 = "sha256-rhEZgHt/jCYmcHMuzwInk+upD3eO86bJ6jVg6nqLkl0=";
        };
      in
      {
        devShells.${system}.default = pkgs.mkShell {
          packages = with pkgs; [
            cargo-udeps
            cargo-outdated
            cargo-expand
            cue
            libusb1
            pkg-config
            toolchain
          ];

          shellHook = ''
            TOP="$(git rev-parse --show-toplevel)"
            export CARGO_HOME="$TOP/.cargo"
          '';
        };

        packages.${system} = with pkgs; {
          default = fujicli;
          inherit fujicli;
        };

        formatter.${system} = treefmt.config.build.wrapper;

        checks.${system} =
          let
            packages = pkgs.lib.mapAttrs' (
              name: pkgs.lib.nameValuePair "package-${name}"
            ) inputs.self.packages.${system};

            devShells = pkgs.lib.mapAttrs' (
              name: pkgs.lib.nameValuePair "devShell-${name}"
            ) inputs.self.devShells.${system};

            formatter.formatting = treefmt.config.build.check inputs.self;
          in
          packages // devShells // formatter;
      }
    );
}
