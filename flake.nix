{
  description = "monotile - a minimalist Wayland compositor";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
  };

  outputs =
    { self, nixpkgs }:
    let
      systems = [
        "x86_64-linux"
        "aarch64-linux"
      ];
      forAllSystems = nixpkgs.lib.genAttrs systems;
    in
    {
      packages = forAllSystems (
        system:
        let
          pkgs = nixpkgs.legacyPackages.${system};
        in
        {
          default = pkgs.rustPlatform.buildRustPackage {
            pname = "monotile";
            version = self.shortRev or self.dirtyShortRev or "unknown";

            src = pkgs.lib.fileset.toSource {
              root = ./.;
              fileset = pkgs.lib.fileset.unions [
                ./src
                ./protocols
                ./defaults
                ./resources
                ./Cargo.toml
                ./Cargo.lock
              ];
            };

            cargoLock = {
              allowBuiltinFetchGit = true;
              lockFile = ./Cargo.lock;
            };

            strictDeps = true;

            preCheck = ''
              export XDG_RUNTIME_DIR=$(mktemp -d)
              export LD_LIBRARY_PATH="${pkgs.lib.makeLibraryPath [ pkgs.wayland ]}"
            '';

            nativeBuildInputs = with pkgs; [
              pkg-config
              autoPatchelfHook
            ];

            buildInputs = with pkgs; [
              libdisplay-info
              libgbm
              libinput
              libxkbcommon
              seatd # libseat
              systemd # libudev
              stdenv.cc.cc.lib # libgcc_s
              wayland
            ];

            runtimeDependencies = with pkgs; [
              libgbm
              libglvnd # libEGL, libGL
              libinput
              libxkbcommon
              seatd # libseat
              systemd # libudev
              wayland
            ];

            meta = {
              description = "A minimalist Wayland compositor inspired by dwm";
              homepage = "https://github.com/lx7/monotile";
              license = pkgs.lib.licenses.gpl3Plus;
              mainProgram = "monotile";
              platforms = pkgs.lib.platforms.linux;
            };
          };
        }
      );

      devShells = forAllSystems (
        system:
        let
          pkgs = nixpkgs.legacyPackages.${system};
          deps = with pkgs; [
            libdisplay-info
            libglvnd # libEGL, libGL
            libinput
            libxkbcommon
            seatd # libseat
            systemd # libudev
            libgbm
            wayland
          ];
        in
        {
          default = pkgs.mkShell {
            hardeningDisable = [ "fortify" ];

            nativeBuildInputs = with pkgs; [
              bashInteractive
              rustc
              cargo
              rustfmt
              clippy
              rust-analyzer
              cargo-llvm-cov
              pkg-config
            ];

            buildInputs = deps;

            env = {
              LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath deps;
              RUST_BACKTRACE = "1";
            };

            shellHook = ''
              export SHELL="${pkgs.bashInteractive}/bin/bash"
              export LLVM_COV="${pkgs.rustc.llvmPackages.llvm}/bin/llvm-cov"
              export LLVM_PROFDATA="${pkgs.rustc.llvmPackages.llvm}/bin/llvm-profdata"
            '';
          };
        }
      );
    };
}
