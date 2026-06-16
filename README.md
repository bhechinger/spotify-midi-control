# spotify-midi-control

`spotify-midi-control` listens for MIDI button presses and sends MPRIS commands to Spotify. It can connect through JACK or directly to PipeWire. The PipeWire backend is usually the easiest option on a modern desktop because it appears as a MIDI input in tools like qpwgraph.

## Running it

For PipeWire, start the program in the dev shell and connect your controller to the `spotify control` MIDI input in qpwgraph:

```sh
nix develop --command cargo run -- \
  --backend pipewire \
  --play-command 176,41,127 \
  --pause-command 176,42,127 \
  --previous-command 176,58,127 \
  --next-command 176,59,127
```

For JACK, run:

```sh
nix develop --command cargo run -- \
  --backend jack \
  --play-command 176,41,127 \
  --pause-command 176,42,127 \
  --previous-command 176,58,127 \
  --next-command 176,59,127
```

The example MIDI bindings used below are control-change messages on channel 0: Play is `176,41,127`, Pause is `176,42,127`, Previous is `176,58,127`, and Next is `176,59,127`. The program requires command values in normal mode; provide them on the command line, through environment variables, or through the Nix module:

```sh
spotify-midi-control \
  --backend pipewire \
  --play-command 176,41,127 \
  --pause-command 176,42,127 \
  --previous-command 176,58,127 \
  --next-command 176,59,127
```

Decimal bytes, hex bytes, and binary bytes are accepted, so `0xB0,41,127` is equivalent to `176,41,127`. Commands must contain one to three bytes.

## Learning buttons

Learning mode prints every MIDI message the program receives. Use it when setting up a controller: start the program, connect the controller in qpwgraph, press the buttons you want to use, and copy the printed values into your config.

```sh
nix develop --command cargo run -- --backend pipewire --learn
```

A learned message looks like this:

```text
midi command: 176,41,127    nix: [ 176 41 127 ]    status: 11 channel: 0
```

Use the `midi command` value with CLI flags or the `nix` value in the NixOS module.

## Flake outputs

This flake can be consumed from another flake. It exports `packages`, `apps`, an `overlays.default`, `nixosModules.default`, and `homeManagerModules.default`.

For a NixOS configuration, add the input and import the module in your host configuration:

```nix
{
  inputs.spotify-midi-control.url = "github:bhechinger/spotify-midi-control";

  outputs = { nixpkgs, spotify-midi-control, ... }: {
    nixosConfigurations.my-host = nixpkgs.lib.nixosSystem {
      system = "x86_64-linux";
      modules = [
        spotify-midi-control.nixosModules.default
        {
          services.spotify-midi-control = {
            enable = true;
            backend = "pipewire";

            midiCommands = {
              play = [ 176 41 127 ];
              pause = [ 176 42 127 ];
              previous = [ 176 58 127 ];
              next = [ 176 59 127 ];
            };
          };
        }
      ];
    };
  };
}
```

For standalone Home Manager, import the Home Manager module instead:

```nix
{
  inputs.spotify-midi-control.url = "github:bhechinger/spotify-midi-control";

  outputs = { nixpkgs, home-manager, spotify-midi-control, ... }:
    let
      system = "x86_64-linux";
      pkgs = nixpkgs.legacyPackages.${system};
    in {
      homeConfigurations.bhechinger = home-manager.lib.homeManagerConfiguration {
        inherit pkgs;
        modules = [
          spotify-midi-control.homeManagerModules.default
          {
            services.spotify-midi-control = {
              enable = true;
              backend = "pipewire";

              midiCommands = {
                play = [ 176 41 127 ];
                pause = [ 176 42 127 ];
                previous = [ 176 58 127 ];
                next = [ 176 59 127 ];
              };
            };
          }
        ];
      };
    };
}
```

The NixOS module adds the package overlay automatically. The Home Manager module passes the package directly, so it does not need a separate overlay. If you only want the package, use `spotify-midi-control.packages.${system}.default` or add `spotify-midi-control.overlays.default` to your own `nixpkgs.overlays`.

## NixOS and Home Manager options

The module exposes a user service at `services.spotify-midi-control`. A typical PipeWire setup looks like this. The `midiCommands` values are required; these example values are kept here as a starting point:

```nix
services.spotify-midi-control = {
  enable = true;
  backend = "pipewire";

  midiCommands = {
    play = [ 176 41 127 ];
    pause = [ 176 42 127 ];
    previous = [ 176 58 127 ];
    next = [ 176 59 127 ];
  };
};
```

To use the service only for discovering button values, temporarily enable learning mode:

```nix
services.spotify-midi-control.learn = true;
```

Then check the user service logs while pressing buttons. Turn learning mode back off after copying the values into `midiCommands`, otherwise the service will only print MIDI messages and will not control Spotify.

To print every received MIDI message while controlling Spotify, enable verbose mode:

```nix
services.spotify-midi-control.verbose = true;
```

The same settings can be supplied through environment variables when running manually:

- `SPOTIFY_MIDI_BACKEND`
- `SPOTIFY_MIDI_CLIENT_NAME`
- `SPOTIFY_MIDI_PIPEWIRE_REMOTE`
- `SPOTIFY_MIDI_PIPEWIRE_TARGET`
- `SPOTIFY_MIDI_LEARN`
- `SPOTIFY_MIDI_VERBOSE`
- `SPOTIFY_MIDI_PLAY_COMMAND`
- `SPOTIFY_MIDI_PAUSE_COMMAND`
- `SPOTIFY_MIDI_PREVIOUS_COMMAND`
- `SPOTIFY_MIDI_NEXT_COMMAND`

## Development checks

Use the flake checks for reproducible native headers and Rust tooling:

```sh
nix flake check
```

For a quick package build without linking a result symlink:

```sh
nix build --no-link
```

Inside the dev shell, stale `target/` artifacts from another Rust compiler can break Cargo. If that happens, use a fresh target directory:

```sh
CARGO_TARGET_DIR=/tmp/spotify-midi-control-target cargo test
```
