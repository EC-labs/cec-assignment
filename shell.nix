{ pkgs ? import <nixpkgs> {} }:
with pkgs; mkShell {
    packages = [
        cargo
        rustc
        rust-analyzer
        pkg-config
        openssl
        cmake
    ];
}
