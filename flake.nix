{
  description = "Interactive MIDI keyboard to musical staff trainer";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs = { self, nixpkgs }:
    let
      supportedSystems = [ "x86_64-linux" "aarch64-linux" ];
      forAllSystems = nixpkgs.lib.genAttrs supportedSystems;
      pkgsFor = system: nixpkgs.legacyPackages.${system};
    in {
      packages = forAllSystems (system: {
        midi-staff-trainer = (pkgsFor system).rustPlatform.buildRustPackage {
          pname = "midi-staff-trainer";
          version = "0.2.0";
          src = ./.;
          cargoLock.lockFile = ./Cargo.lock;
          nativeBuildInputs = with (pkgsFor system); [ pkg-config ];
          buildInputs = with (pkgsFor system); [
            alsa-lib
            libxkbcommon
            wayland
            libGL
            libx11
            libxcursor
            libxrandr
            libxi
          ];
        };
        default = self.packages.${system}.midi-staff-trainer;
      });

      devShells = forAllSystems (system:
        let pkgs = pkgsFor system; in {
          default = pkgs.mkShell {
            nativeBuildInputs = with pkgs; [ rustc cargo clippy rustfmt pkg-config ];
            buildInputs = with pkgs; [
              alsa-lib
              libxkbcommon
              wayland
              libGL
              libx11
              libxcursor
              libxrandr
              libxi
            ];
            LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath [
              pkgs.libGL
              pkgs.wayland
              pkgs.libxkbcommon
              pkgs.libx11
              pkgs.libxcursor
              pkgs.libxrandr
              pkgs.libxi
            ];
          };
        }
      );
    };
}
