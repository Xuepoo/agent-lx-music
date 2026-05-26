{
  description = "A terminal-native music CLI replacing lx-music-desktop, powered by Agentic intelligence.";

  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, flake-utils }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = nixpkgs.legacyPackages.${system};
      in
      {
        packages.default = pkgs.rustPlatform.buildRustPackage {
          pname = "agent-lx-music";
          version = "0.1.0";
          src = ./.;

          cargoLock = {
            lockFile = ./Cargo.lock;
          };

          nativeBuildInputs = with pkgs; [ pkg-config ];
          buildInputs = with pkgs; [ alsa-lib mpv ];

          meta = with pkgs.lib; {
            description = "A terminal-native music CLI replacing lx-music-desktop, powered by Agentic intelligence.";
            homepage = "https://github.com/Xuepoo/agent-lx-music";
            license = licenses.mit;
            maintainers = [ ];
          };
        };

        devShells.default = pkgs.mkShell {
          buildInputs = with pkgs; [
            cargo
            rustc
            pkg-config
            alsa-lib
            mpv
          ];
        };
      }
    );
}
