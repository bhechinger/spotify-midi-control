{
  lib,
  pkgs,
  config,
  spotify-midi-control-package ? pkgs.spotify-midi-control,
  systemd-service-style ? "nixos",
  ...
}:
with lib;
let
  cfg = config.services.spotify-midi-control;
  midiCommandType = types.addCheck (types.nonEmptyListOf (types.ints.between 0 255)) (
    command: length command <= 3
  );
  midiCommand = command: concatMapStringsSep "," toString command;
  command = concatStringsSep " " (
    [
      "${cfg.package}/bin/spotify-midi-control"
      "--backend ${escapeShellArg cfg.backend}"
      "--client-name ${escapeShellArg cfg.clientName}"
    ]
    ++ optional cfg.learn "--learn"
    ++ optional cfg.verbose "--verbose"
    ++ optionals (!cfg.learn) [
      "--play-command ${escapeShellArg (midiCommand cfg.midiCommands.play)}"
      "--pause-command ${escapeShellArg (midiCommand cfg.midiCommands.pause)}"
      "--previous-command ${escapeShellArg (midiCommand cfg.midiCommands.previous)}"
      "--next-command ${escapeShellArg (midiCommand cfg.midiCommands.next)}"
    ]
    ++ optionals (cfg.backend == "pipewire") (
      optional (cfg.pipewireRemote != null) "--pipewire-remote ${escapeShellArg cfg.pipewireRemote}"
      ++ optional (cfg.pipewireTarget != null) "--pipewire-target ${toString cfg.pipewireTarget}"
    )
  );
  service =
    if systemd-service-style == "home-manager" then
      {
        Unit = {
          Wants = optional (cfg.backend == "pipewire") "pipewire.service";
          After = optional (cfg.backend == "pipewire") "pipewire.service";
        };

        Service = {
          ExecStart = command;
        };

        Install = {
          WantedBy = [ "default.target" ];
        };
      }
    else
      {
        wants = optional (cfg.backend == "pipewire") "pipewire.service";
        after = optional (cfg.backend == "pipewire") "pipewire.service";
        wantedBy = [ "default.target" ];
        serviceConfig.ExecStart = command;
      };
in
{
  options.services.spotify-midi-control = {
    enable = mkEnableOption "Spotify MIDI Control";

    backend = mkOption {
      type = types.enum [
        "jack"
        "pipewire"
      ];
      default = "jack";
      description = "MIDI backend to use.";
    };

    clientName = mkOption {
      type = types.str;
      default = "spotify control";
      description = "Client name advertised by the selected MIDI backend.";
    };

    package = mkOption {
      type = types.package;
      default = spotify-midi-control-package;
      defaultText = literalExpression "pkgs.spotify-midi-control";
      description = "Package providing the spotify-midi-control executable.";
    };

    pipewireRemote = mkOption {
      type = types.nullOr types.str;
      default = null;
      description = "Optional PipeWire remote daemon name.";
    };

    pipewireTarget = mkOption {
      type = types.nullOr types.int;
      default = null;
      description = "Optional PipeWire target node id.";
    };

    learn = mkOption {
      type = types.bool;
      default = false;
      description = "Print incoming MIDI command bytes instead of controlling Spotify.";
    };

    verbose = mkOption {
      type = types.bool;
      default = false;
      description = "Print every received MIDI message while controlling Spotify.";
    };

    midiCommands = {
      play = mkOption {
        type = midiCommandType;
        description = "MIDI bytes that trigger Play.";
      };

      pause = mkOption {
        type = midiCommandType;
        description = "MIDI bytes that trigger Pause.";
      };

      previous = mkOption {
        type = midiCommandType;
        description = "MIDI bytes that trigger Previous.";
      };

      next = mkOption {
        type = midiCommandType;
        description = "MIDI bytes that trigger Next.";
      };
    };
  };

  config = mkIf cfg.enable {
    systemd.user.services.spotify-midi-control = service;
  };
}
