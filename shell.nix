{
  mkShell,
  callPackage,
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

  LIBCLANG_PATH = "${llvmPackages.libclang.lib}/lib";
  BINDGEN_EXTRA_CLANG_ARGS = "-isystem ${stdenv.cc.libc.dev}/include";
}
