{
  description = "Spotify MIDI Control";

  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixos-unstable";

    home-manager = {
      url = "github:nix-community/home-manager";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    {
      self,
      nixpkgs,
      home-manager,
    }:
    let
      supportedSystems = [ "x86_64-linux" ];
      forAllSystems = nixpkgs.lib.genAttrs supportedSystems;
      pkgsFor = nixpkgs.legacyPackages;
      testServiceConfig = {
        services.spotify-midi-control = {
          enable = true;
          backend = "pipewire";
          midiCommands = {
            play = [
              176
              41
              127
            ];
            pause = [
              176
              42
              127
            ];
            previous = [
              176
              58
              127
            ];
            next = [
              176
              59
              127
            ];
          };
        };
      };
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

      checks = forAllSystems (
        system:
        let
          pkgs = pkgsFor.${system};
          nixosExecStart =
            (nixpkgs.lib.nixosSystem {
              inherit system;
              modules = [
                self.nixosModules.default
                testServiceConfig
              ];
            }).config.systemd.user.services.spotify-midi-control.serviceConfig.ExecStart;
          homeManagerExecStartValue =
            (home-manager.lib.homeManagerConfiguration {
              inherit pkgs;
              modules = [
                self.homeManagerModules.default
                {
                  home.username = "spotify-midi-control-test";
                  home.homeDirectory = "/tmp/spotify-midi-control-test";
                  home.stateVersion = "26.05";
                }
                testServiceConfig
              ];
            }).config.systemd.user.services.spotify-midi-control.Service.ExecStart;
          homeManagerExecStart =
            if builtins.isList homeManagerExecStartValue then
              nixpkgs.lib.concatStringsSep "\n" homeManagerExecStartValue
            else
              homeManagerExecStartValue;
        in
        {
          nixos-module = pkgs.runCommand "spotify-midi-control-nixos-module-check" {
            execStart = nixosExecStart;
          } "test -n \"$execStart\"; touch \"$out\"";

          home-manager-module = pkgs.runCommand "spotify-midi-control-home-manager-module-check" {
            execStart = homeManagerExecStart;
          } "test -n \"$execStart\"; touch \"$out\"";
        }
      );

      overlays.default = final: _prev: {
        spotify-midi-control = self.packages.${final.system}.spotify-midi-control;
      };

      nixosModules.default =
        { pkgs, ... }:
        {
          imports = [ ./module.nix ];
          nixpkgs.overlays = [ self.overlays.default ];
          _module.args.spotify-midi-control-package = self.packages.${pkgs.system}.spotify-midi-control;
          _module.args.systemd-service-style = "nixos";
        };

      homeManagerModules.default =
        { pkgs, ... }:
        {
          imports = [ ./module.nix ];
          _module.args.spotify-midi-control-package = self.packages.${pkgs.system}.spotify-midi-control;
          _module.args.systemd-service-style = "home-manager";
        };
    };
}
