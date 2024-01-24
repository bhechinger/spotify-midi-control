{ pkgs ? import <nixpkgs> { } }:
pkgs.rustPlatform.buildRustPackage rec {
  pname = "spotify-midi-control";
  version = "0.1";
  cargoLock.lockFile = ./Cargo.lock;
  src = pkgs.lib.cleanSource ./.;

  buildInputs = with pkgs; [
    pkg-config
    libjack2
  ];

  nativeBuildInputs = with pkgs; [
    pkg-config
    libjack2
  ];
}
