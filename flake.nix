{
  description = "Spotify MIDI Control";

  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixos-unstable";

    home-manager = {
      url = "github:nix-community/home-manager";
      inputs.nixpkgs.follows = "nixpkgs";
    };

    npm-package = {
      url = "github:netbrain/npm-package";
      inputs.nixpkgs.follows = "nixpkgs-npm-package";
    };

    nixpkgs-npm-package.url = "github:NixOS/nixpkgs/ba487dbc9d04e0634c64e3b1f0d25839a0a68246";
  };

  outputs =
    {
      self,
      nixpkgs,
      home-manager,
      npm-package,
      ...
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

      devShells = forAllSystems (
        system:
        let
          pkgs = pkgsFor.${system};
        in
        {
          default = (pkgs.callPackage ./shell.nix { }).overrideAttrs (oldAttrs: {
            nativeBuildInputs = (oldAttrs.nativeBuildInputs or [ ]) ++ [
              (npm-package.lib.${system}.npmPackage {
                name = "greptile";
                version = "3.0.7";
              })
            ];
          });
        }
      );

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
          tooLongMidiCommandRejected =
            let
              badExecStart =
                (nixpkgs.lib.nixosSystem {
                  inherit system;
                  modules = [
                    self.nixosModules.default
                    testServiceConfig
                    {
                      services.spotify-midi-control.midiCommands.play = [
                        176
                        41
                        127
                        1
                      ];
                    }
                  ];
                }).config.systemd.user.services.spotify-midi-control.serviceConfig.ExecStart;
            in
            !(builtins.tryEval (builtins.deepSeq badExecStart badExecStart)).success;
        in
        {
          package = self.packages.${system}.spotify-midi-control;

          rustfmt =
            pkgs.runCommand "spotify-midi-control-rustfmt-check"
              {
                nativeBuildInputs = [ pkgs.rustfmt ];
              }
              ''
                cp -r ${./src} src
                chmod -R u+w src
                rustfmt --edition 2021 --check src/*.rs
                touch "$out"
              '';

          nixos-module = pkgs.runCommand "spotify-midi-control-nixos-module-check" {
            execStart = nixosExecStart;
          } "test -n \"$execStart\"; touch \"$out\"";

          home-manager-module = pkgs.runCommand "spotify-midi-control-home-manager-module-check" {
            execStart = homeManagerExecStart;
          } "test -n \"$execStart\"; touch \"$out\"";

          nixos-module-rejects-too-long-midi-command =
            pkgs.runCommand "spotify-midi-control-nixos-module-rejects-too-long-midi-command"
              {
                rejected = if tooLongMidiCommandRejected then "1" else "0";
              }
              "test \"$rejected\" = 1; touch \"$out\"";
        }
      );

      overlays.default = final: _prev: {
        spotify-midi-control = self.packages.${final.stdenv.hostPlatform.system}.spotify-midi-control;
      };

      nixosModules.default =
        { pkgs, ... }:
        {
          imports = [ ./module.nix ];
          nixpkgs.overlays = [ self.overlays.default ];
          _module.args.spotify-midi-control-package =
            self.packages.${pkgs.stdenv.hostPlatform.system}.spotify-midi-control;
          _module.args.systemd-service-style = "nixos";
        };

      homeManagerModules.default =
        { pkgs, ... }:
        {
          imports = [ ./module.nix ];
          _module.args.spotify-midi-control-package =
            self.packages.${pkgs.stdenv.hostPlatform.system}.spotify-midi-control;
          _module.args.systemd-service-style = "home-manager";
        };
    };
}
