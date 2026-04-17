{
  description = "Nixified dev container";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-25.05";
    systemflake = {
      url = "github:mikeshepherd/nixos?ref=refs/tags/fixed-2025.11.27.1";
    };
    devflake = {
      url = "github:mikeshepherd/devflake";
      inputs.nixpkgs.follows = "nixpkgs";
      inputs.systemflake.follows = "systemflake";
    };
    nixpkgs-unstable.url = "github:NixOS/nixpkgs/master";

    fenix = {
      url = "github:nix-community/fenix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    crane.url = "github:ipetkov/crane";
  };

  outputs =
    {
      nixpkgs,
      nixpkgs-unstable,
      self,
      devflake,
      systemflake,
      fenix,
      crane,
      ...
    }@inputs:
    let
      system = "x86_64-linux";
      hostname = "codeServerVM";
      pkgs = import nixpkgs {
        inherit system;
        overlays = [ fenix.overlays.default ];
      };

      craneLib =
        (crane.mkLib nixpkgs.legacyPackages.${system}).overrideToolchain
          fenix.packages.${system}.stable.toolchain;
    in
    {
      packages.${system}.default = craneLib.buildPackage {
        src = ./.;
      };

      inherit self systemflake;

      devShells.${system}.default =
        devflake.lib.mkDevShell systemflake.nixosConfigurations.${hostname}.config
          rec {
            name = "gpg-bridge";
            hostUser = "mike";
            devContainerSettings = {
              privileged = true;
            };
            presets = [ "rust" ];
            extraPackages = [
              nixpkgs-unstable.legacyPackages.${system}.podman-compose
            ];
            devEnvModules = [
              {
                packages = [
                  (pkgs.fenix.complete.withComponents [
                    "cargo"
                    "clippy"
                    "rust-src"
                    "rustc"
                    "rustfmt"
                  ])
                  pkgs.rust-analyzer-nightly
                ];
              }
            ];
            additionalMounts = [
              "source=/home/${hostUser}/.cache,target=/root/.cache,type=bind"
              "source=${name}-empty-dir,target=/root/.config/environment.d,type=volume"
            ];
          };
    };
}
