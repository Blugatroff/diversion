{
    inputs = {
        nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
        flake-utils.url = "github:numtide/flake-utils";
        rust-overlay = {
            url = "github:oxalica/rust-overlay";
            inputs = {
                nixpkgs.follows = "nixpkgs";
                flake-utils.follows = "flake-utils";
            };
        };
    };
    outputs = { self, nixpkgs, flake-utils, rust-overlay }:
        flake-utils.lib.eachDefaultSystem
            (system: let
                overlays = [ (import rust-overlay) ];
                pkgs = import nixpkgs {
                    inherit system overlays;
                };
                nativeBuildInputs = with pkgs; [ 
                    pkgs.rust-bin.stable.latest.default 
                    pkg-config 
                    rust-analyzer
                ];
            in {
                devShells.default = pkgs.mkShell {
                    buildInputs = [ pkgs.lua5_4 ];
                    inherit nativeBuildInputs;
                };
            });
}

