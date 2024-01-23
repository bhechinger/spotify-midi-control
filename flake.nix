{
  description = "spotify-midi-control";

  inputs.nixpkgs.url = "nixpkgs/nixos-unstable";
  inputs.flake-utils.url = "github:numtide/flake-utils";

  outputs = { self, nixpkgs, flake-utils }:
    # Add dependencies that are only needed for development
    flake-utils.lib.eachDefaultSystem
      (system:
        let
          pkgs = import nixpkgs {
            inherit system;
          };
        in
        {
          devShells.default = let p = pkgs; in
            pkgs.mkShell {
              buildInputs =
                [
                  p.cargo
                  p.rustc
		  p.pkg-config
		  p.libjack2
                ];
            };
        });
}

