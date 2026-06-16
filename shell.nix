{
  mkShell,
  callPackage,
  rustPlatform,
  clippy,
  rust-analyzer,
  libjack2,
  pipewire,
  llvmPackages,
  stdenv,
}:
mkShell {
  # Get dependencies from the main package
  inputsFrom = [ (callPackage ./default.nix { }) ];
  # Additional tooling
  buildInputs = [
    libjack2
    pipewire
    llvmPackages.libclang
  ];
  nativeBuildInputs = [
    clippy
    rust-analyzer
  ];

  LIBCLANG_PATH = "${llvmPackages.libclang.lib}/lib";
  BINDGEN_EXTRA_CLANG_ARGS = "-isystem ${stdenv.cc.libc.dev}/include";
  RUST_SRC_PATH = "${rustPlatform.rustLibSrc}";
}
