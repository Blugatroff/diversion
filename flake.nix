{
    inputs = {
        nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
        rust-overlay = {
            url = "github:oxalica/rust-overlay";
            inputs.nixpkgs.follows = "nixpkgs";
        };
    };
    outputs = { self, nixpkgs, rust-overlay }:
        let
            system = "x86_64-linux";
            overlays = [ (import rust-overlay) ];
            pkgs = import nixpkgs {
                inherit system overlays;
            };
            nativeBuildInputs = with pkgs; [
                pkgs.rust-bin.stable.latest.default
                pkg-config
                rust-analyzer
            ];
            buildInputs = [ pkgs.lua5_4 ];
        in {
            devShells.default = pkgs.mkShell {
                inherit nativeBuildInputs buildInputs;
            };
            packages.x86_64-linux.default = pkgs.rustPlatform.buildRustPackage {
                inherit nativeBuildInputs buildInputs;
                pname = "diversion";
                version = "1.0.0";
                src = ./.;

                cargoLock = {
                    lockFile = ./Cargo.lock;
                };
            };
        };
}

