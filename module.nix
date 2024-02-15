{ lib, pkgs, config, ... }:
with lib;
let
  # Shorter name to access final settings a
  # user of hello.nix module HAS ACTUALLY SET.
  # cfg is a typical convention.
  cfg = config.services.spotify-midi-control;
in {
  # Declare what settings a user of this "hello.nix" module CAN SET.
  options.services.spotify-midi-control = {
    enable = mkEnableOption "Spotify MIDI Control";
    midi_config = {
      thing1 = mkOption {
        type = types.integer;
        default = 128;
      };
      status = mkOption {
        type = types.integer;
        default = 0b1011;
      };
      channel = mkOption {
        type = types.integer;
        default = 0b0;
      };
      controls = {
        play = mkOption {
          type = types.integer;
          default = 41;
        };
        pause = mkOption {
          type = types.integer;
          default = 42;
        };
        previous = mkOption {
          type = types.integer;
          default = 58;
        };
        next = mkOption {
          type = types.integer;
          default = 59;
        };
      };
    };
  };

  # Define what other settings, services and resources should be active IF
  # a user of this "hello.nix" module ENABLED this module
  # by setting "services.hello.enable = true;".
  config = mkIf cfg.enable {
    systemd.services.spotify-midi-control = {
      wantedBy = [ "multi-user.target" ];
      serviceConfig.ExecStart = "${pkgs.spotify-midi-control}/bin/spotify-midi-config";
    };
  };
}