{lib, rustPlatform, pkg-config, libjack2}:
rustPlatform.buildRustPackage rec {
  pname = "spotify-midi-control";
  version = "0.1";
  cargoLock.lockFile = ./Cargo.lock;
  src = lib.cleanSource ./.;

  buildInputs = [
    libjack2
  ];

  nativeBuildInputs = [
    pkg-config
  ];
}
