{
  lib,
  rustPlatform,
  pkg-config,
  libjack2,
  pipewire,
  llvmPackages,
  stdenv,
}:
rustPlatform.buildRustPackage {
  pname = "spotify-midi-control";
  version = "0.1.0";
  cargoLock.lockFile = ./Cargo.lock;
  src = lib.cleanSource ./.;

  buildInputs = [
    libjack2
    pipewire
  ];

  nativeBuildInputs = [
    pkg-config
    llvmPackages.libclang
  ];

  LIBCLANG_PATH = "${llvmPackages.libclang.lib}/lib";
  BINDGEN_EXTRA_CLANG_ARGS = "-isystem ${stdenv.cc.libc.dev}/include";
}
