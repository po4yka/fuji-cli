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
    let
      systems = [
        "aarch64-darwin"
        "aarch64-linux"
        "x86_64-darwin"
        "x86_64-linux"
      ];
      forAllSystems = inputs.nixpkgs.lib.genAttrs systems;
    in
    {
      overlays.default =
        final: _prev:
        let
          manifest = builtins.fromTOML (builtins.readFile ./Cargo.toml);
          toolchain = inputs.fenix.packages.${final.system}.fromToolchainFile {
            file = ./rust-toolchain.toml;
            sha256 = "sha256-rhEZgHt/jCYmcHMuzwInk+upD3eO86bJ6jVg6nqLkl0=";
          };
          rustPlatform = final.makeRustPlatform {
            cargo = toolchain;
            rustc = toolchain;
          };
        in
        {
          fujicliToolchain = toolchain;

          fujicli = rustPlatform.buildRustPackage {
            pname = manifest.package.name;
            version = manifest.package.version;

            src = ./.;
            cargoLock = {
              lockFile = ./Cargo.lock;
              extraRegistries = {
                "https://github.com/rust-lang/crates.io-index" = "https://static.crates.io/crates";
              };
            };

            nativeBuildInputs = with final; [
              cue
              pkg-config
            ];

            buildInputs = with final; [ libusb1 ];
          };
        };

      packages = forAllSystems (
        system:
        let
          pkgs = import inputs.nixpkgs {
            inherit system;
            overlays = [ inputs.self.overlays.default ];
          };
        in
        {
          default = pkgs.fujicli;
          inherit (pkgs) fujicli;
        }
      );

      devShells = forAllSystems (
        system:
        let
          pkgs = import inputs.nixpkgs {
            inherit system;
            overlays = [ inputs.self.overlays.default ];
          };
        in
        {
          default = pkgs.mkShell {
            packages = with pkgs; [
              cargo-expand
              cargo-outdated
              cargo-udeps
              cue
              fujicliToolchain
              libusb1
              pkg-config
            ];

            shellHook = ''
              TOP="$(git rev-parse --show-toplevel)"
              export CARGO_HOME="$TOP/.cargo"
            '';
          };
        }
      );

      formatter = forAllSystems (
        system:
        let
          pkgs = import inputs.nixpkgs {
            inherit system;
            overlays = [ inputs.self.overlays.default ];
          };
          treefmt = inputs.treefmt-nix.lib.evalModule pkgs ./treefmt.nix;
        in
        treefmt.config.build.wrapper
      );

      checks = forAllSystems (
        system:
        let
          pkgs = import inputs.nixpkgs {
            inherit system;
            overlays = [ inputs.self.overlays.default ];
          };
          treefmt = inputs.treefmt-nix.lib.evalModule pkgs ./treefmt.nix;
        in
        {
          package-fujicli = inputs.self.packages.${system}.fujicli;
          formatting = treefmt.config.build.check inputs.self;
          devShell-default = inputs.self.devShells.${system}.default;
        }
      );
    };
}
