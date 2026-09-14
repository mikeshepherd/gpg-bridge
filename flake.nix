{
  description = "Nixified dev container";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-25.11";
    crane.url = "github:ipetkov/crane";

    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, crane, flake-utils, ... }:
      flake-utils.lib.eachDefaultSystem (system:
        let
        rustBuildTargetTriple = "x86_64-pc-windows-gnu";

        # Our windows cross package set.
        pkgs-cross-mingw = import pkgs.path {
          localSystem = system;
          crossSystem = {
              config = "x86_64-w64-mingw32";
            };
        };

        # Our windows cross compiler plus
        # the required libraries and headers.
        mingw_w64_cc = pkgs-cross-mingw.stdenv.cc;
        mingw_w64 = pkgs-cross-mingw.windows.mingw_w64;
        mingw_w64_pthreads_w_static = pkgs-cross-mingw.windows.pthreads.overrideAttrs (oldAttrs: {
          # TODO: Remove once / if changed successfully upstreamed.
          configureFlags = (oldAttrs.configureFlags or []) ++ [
            # Rustc require 'libpthread.a' when targeting 'x86_64-pc-windows-gnu'.
            # Enabling this makes it work out of the box instead of failing.
            "--enable-static"
          ];
        });
        rustcWithoutDocs = pkgs':
          let
            buildPkgs = pkgs'.pkgsBuildBuild;
            rustcUnwrapped = pkgs'.rustc.unwrapped;
            defaultArgs = buildPkgs.lib.optionalString (
              with rustcUnwrapped.stdenv.targetPlatform; isMusl && !isStatic
            ) "-C target-feature=-crt-static";
            mkRustWrapper = program: extraFlags: buildPkgs.writeShellScript "${program}-without-docs-wrapper" ''
              defaultSysroot=(--sysroot ${rustcUnwrapped})
              extraBefore=(${defaultArgs} "''${defaultSysroot[@]}")
              extraAfter=(${extraFlags})

              exec ${rustcUnwrapped}/bin/${program} "''${extraBefore[@]}" "$@" "''${extraAfter[@]}"
            '';
          in
          buildPkgs.runCommand "${rustcUnwrapped.pname}-without-docs-wrapper-${rustcUnwrapped.version}" {
            preferLocalBuild = true;
            strictDeps = true;
          } ''
            mkdir -p $out/bin
            ln -s ${rustcUnwrapped}/bin/* $out/bin
            rm $out/bin/{rustc,rustdoc}
            ln -s ${mkRustWrapper "rustc" "$NIX_RUSTFLAGS"} $out/bin/rustc
            ln -s ${mkRustWrapper "rustdoc" "$NIX_RUSTDOCFLAGS"} $out/bin/rustdoc
          '';
        crossToolchainWithoutDocs = pkgs':
          let
            buildPkgs = pkgs'.pkgsBuildBuild;
            rustc = rustcWithoutDocs pkgs';
          in
          buildPkgs.symlinkJoin {
            name = "rust-toolchain-without-docs";
            paths = [ pkgs'.cargo pkgs'.clippy rustc pkgs'.rustfmt ];
          };
          pkgs = nixpkgs.legacyPackages.${system};
          craneLib = crane.mkLib pkgs;
          craneLibCross = (crane.mkLib pkgs-cross-mingw).overrideToolchain crossToolchainWithoutDocs;
          src = craneLib.cleanCargoSource self;
          commonArgs = {
            inherit src;
            strictDeps = true;
          };
          windowsPackage = craneLibCross.buildPackage (commonArgs // {
            cargoArtifacts = craneLibCross.buildDepsOnly commonArgs;
            doCheck = false;
          });
          windowsArchive = pkgs.runCommand "gpg-bridge-windows-x86_64.zip" {
            nativeBuildInputs = [ pkgs.zip ];
          } ''
            mkdir -p staging
            cp ${windowsPackage}/bin/gpg-bridge.exe staging/
            cp ${self}/contrib/windows/install-service.ps1 staging/
            cp ${self}/contrib/windows/request-step-ca-server-certificate.ps1 staging/
            cp ${self}/contrib/windows/renew-step-ca-server-certificate.ps1 staging/
            cp ${self}/contrib/windows/install-certificate-renewal-service.ps1 staging/
            cd staging
            zip -X -r "$out" .
          '';
          # Read the file relative to the flake's root
          overrides = (builtins.fromTOML (builtins.readFile (self + "/rust-toolchain.toml")));
        in
        {
          packages = {
            default = craneLib.buildPackage (commonArgs // {
              cargoArtifacts = craneLib.buildDepsOnly commonArgs;
            });
          } // pkgs.lib.optionalAttrs (system == "x86_64-linux") {
            windows-x86_64 = windowsPackage;
            windows-archive = windowsArchive;
          };

          devShells.default = pkgs.mkShell rec {
            nativeBuildInputs = [ pkgs.pkg-config ];
            buildInputs = with pkgs; [
              clang
              llvmPackages.bintools
              rustup
              mingw_w64_cc
            ];

            RUSTC_VERSION = overrides.toolchain.channel;

            # https://github.com/rust-lang/rust-bindgen#environment-variables
            LIBCLANG_PATH = pkgs.lib.makeLibraryPath [ pkgs.llvmPackages_latest.libclang.lib ];

            shellHook = ''
              export PATH=$PATH:''${CARGO_HOME:-~/.cargo}/bin
              export PATH=$PATH:''${RUSTUP_HOME:-~/.rustup}/toolchains/$RUSTC_VERSION-x86_64-unknown-linux-gnu/bin/
              # Ensures our windows target is added via rustup.
              rustup target add "${rustBuildTargetTriple}"
            '';

            # Add precompiled library to rustc search path
            RUSTFLAGS = (builtins.map (a: ''-L ${a}/lib'') [
              # add libraries here (e.g. pkgs.libvmi)
              mingw_w64
              mingw_w64_pthreads_w_static
            ]);

            LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath (buildInputs ++ nativeBuildInputs);


            # Add glibc, clang, glib, and other headers to bindgen search path
            BINDGEN_EXTRA_CLANG_ARGS =
            # Includes normal include path
            (builtins.map (a: ''-I"${a}/include"'') [
              # add dev libraries here (e.g. pkgs.libvmi.dev)
              pkgs.glibc.dev
            ])
            # Includes with special directory paths
            ++ [
              ''-I"${pkgs.llvmPackages_latest.libclang.lib}/lib/clang/${pkgs.llvmPackages_latest.libclang.version}/include"''
              ''-I"${pkgs.glib.dev}/include/glib-2.0"''
              ''-I${pkgs.glib.out}/lib/glib-2.0/include/''
            ];
          };
        }
      );
}
