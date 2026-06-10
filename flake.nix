{
  description = "Spotify MIDI Control";

  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixos-unstable";
  };

  outputs =
    { self, nixpkgs }:
    let
      supportedSystems = [ "x86_64-linux" ];
      forAllSystems = nixpkgs.lib.genAttrs supportedSystems;
      pkgsFor = nixpkgs.legacyPackages;
    in
    {
      packages = forAllSystems (system: {
        spotify-midi-control = pkgsFor.${system}.callPackage ./default.nix { };
        default = self.packages.${system}.spotify-midi-control;
      });

      apps = forAllSystems (system: {
        spotify-midi-control = {
          type = "app";
          program = "${self.packages.${system}.spotify-midi-control}/bin/spotify-midi-control";
        };
        default = self.apps.${system}.spotify-midi-control;
      });

      devShells = forAllSystems (system: {
        default = pkgsFor.${system}.callPackage ./shell.nix { };
      });

      overlays.default = final: _prev: {
        spotify-midi-control = self.packages.${final.system}.spotify-midi-control;
      };

      nixosModules.default =
        { pkgs, ... }:
        {
          imports = [ ./module.nix ];
          nixpkgs.overlays = [ self.overlays.default ];
          _module.args.spotify-midi-control-package = self.packages.${pkgs.system}.spotify-midi-control;
        };

      homeManagerModules.default =
        { pkgs, ... }:
        {
          imports = [ ./module.nix ];
          _module.args.spotify-midi-control-package = self.packages.${pkgs.system}.spotify-midi-control;
        };
    };
}
