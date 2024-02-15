{mkShell, callPackage, libjack2}:
mkShell {
  # Get dependencies from the main package
  inputsFrom = [ (callPackage ./default.nix { }) ];
  # Additional tooling
  buildInputs = [
    libjack2
  ];
}
