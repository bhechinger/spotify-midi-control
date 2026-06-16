# Hardening Review Fixes Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Address the review findings around PipeWire parsing safety, DBus robustness, Nix validation, event-loss visibility, dead code, and contributor verification.

**Architecture:** Keep the realtime callback shape: copy MIDI bytes into an `rtrb` queue and do all logging/DBus work off the callback thread. Replace or constrain unsafe SPA POD parsing behind one small parser boundary, make controller actions typed, and align Nix/module validation with the Rust CLI.

**Tech Stack:** Rust 2021, `pipewire`/`libspa`, JACK, `rtrb`, `rustbus`, Clap, Nix flakes, Home Manager/NixOS modules.

---

## File Structure

- Modify `src/main.rs`: action enum, controller logging, drop counters, PipeWire parse tests, CLI verbosity.
- Modify `src/spotify.rs`: typed action input, DBus timeout/error propagation, remove pass-through sender.
- Modify `src/midi.rs`: safer/clearer MIDI debug output if needed by tests.
- Modify `module.nix`: enforce 1..3 MIDI bytes and add optional verbose flag if implemented.
- Modify `README.md`: document `--verbose`, validation, and contributor verification.
- Modify `flake.nix`: add `cargo test`/format checks if not already covered by package build.
- Optionally create `.cargo/config.toml`: only if direct Cargo needs stable `LIBCLANG_PATH`/PipeWire header behavior outside `nix develop`; otherwise do not add it.

## Task 1: Pin Down Existing Behavior With Tests

**Files:**
- Modify: `src/main.rs`
- Modify: `module.nix`

- [ ] Add tests for MIDI command parsing in `src/main.rs` test module.

```rust
#[test]
fn parse_midi_command_accepts_one_to_three_bytes() {
    assert_eq!(parse_midi_command("176").unwrap().len, 1);
    assert_eq!(parse_midi_command("176,41").unwrap().len, 2);
    assert_eq!(parse_midi_command("0xB0,0b101001,127").unwrap().bytes, [176, 41, 127]);
}

#[test]
fn parse_midi_command_rejects_empty_or_too_long_commands() {
    assert!(parse_midi_command("").is_err());
    assert!(parse_midi_command("176,41,127,1").is_err());
}
```

- [ ] Run `cargo test`.
  Expected: new tests pass.

- [ ] Add a Nix check that evaluates a too-long MIDI command and expects failure. Put it beside the existing module checks in `flake.nix`.
  Expected command: `nix flake check --no-build`.

- [ ] Commit.

```bash
git add src/main.rs flake.nix
git commit -m "test: cover midi command validation"
```

## Task 2: Replace String Actions With A Typed Enum

**Files:**
- Modify: `src/main.rs`
- Modify: `src/spotify.rs`

- [ ] Add an action enum in `src/main.rs` near `MidiBindings`.

```rust
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SpotifyAction {
    Play,
    Pause,
    Previous,
    Next,
}

impl SpotifyAction {
    fn mpris_method(self) -> &'static str {
        match self {
            SpotifyAction::Play => "Play",
            SpotifyAction::Pause => "Pause",
            SpotifyAction::Previous => "Previous",
            SpotifyAction::Next => "Next",
        }
    }
}
```

- [ ] Change `MidiBindings::action_for` to return `Option<SpotifyAction>`.

```rust
fn action_for(&self, midi: &midi::MidiCopy) -> Option<SpotifyAction> {
    if self.play.matches(midi) {
        Some(SpotifyAction::Play)
    } else if self.pause.matches(midi) {
        Some(SpotifyAction::Pause)
    } else if self.previous.matches(midi) {
        Some(SpotifyAction::Previous)
    } else if self.next.matches(midi) {
        Some(SpotifyAction::Next)
    } else {
        None
    }
}
```

- [ ] Remove `sender` from `Spotify` in `src/spotify.rs`; change `handle_midi` to `handle_action(&mut self, action: SpotifyAction)`.
  If keeping `SpotifyAction` private in `main.rs` is awkward, move it to `spotify.rs` and import it in `main.rs`.

- [ ] Run `cargo test`.

- [ ] Commit.

```bash
git add src/main.rs src/spotify.rs
git commit -m "refactor: use typed spotify actions"
```

## Task 3: Make DBus Failures Non-Panicking And Bounded

**Files:**
- Modify: `src/spotify.rs`
- Modify: `src/main.rs`

- [ ] Replace `Timeout::Infinite` with a finite timeout in `Spotify::new`.

```rust
const DBUS_TIMEOUT: Timeout = Timeout::Duration(std::time::Duration::from_secs(2));
```

Use it for `send_hello`.

- [ ] Replace `.write_all().unwrap()` with error propagation.

```rust
self.connection
    .send
    .send_message(&call)?
    .write_all()?;
```

If the exact error type differs, introduce a local error enum:

```rust
#[derive(Debug)]
pub enum SpotifyError {
    Connection(rustbus::connection::Error),
    Io(std::io::Error),
}
```

and implement `From` plus `Display`/`Error`.

- [ ] Update controller logging in `src/main.rs` to report the typed action and keep running after failed sends.

- [ ] Run `cargo test`.

- [ ] Commit.

```bash
git add src/spotify.rs src/main.rs
git commit -m "fix: return dbus write errors instead of panicking"
```

## Task 4: Fix PipeWire SPA Sequence Parsing Safety

**Files:**
- Modify: `src/main.rs`

- [ ] First look for a safe `libspa` sequence/control iterator in the installed crate docs/source.
  Use:

```bash
rg -n "Sequence|Control|SPA_CONTROL_Midi|pod_control|deserialize" ~/.cargo/registry/src -g '*.rs'
```

- [ ] If a safe iterator exists, replace `for_each_spa_midi_sequence` with it and delete manual pointer dereferences.

- [ ] If no safe iterator exists, keep the manual parser but make it unaligned and fully checked:
  - derive the body as `let body = pod.body().cast::<u8>()`
  - before reading a control, check `offset <= pod_size - size_of::<spa_pod_control>()`
  - read with `std::ptr::read_unaligned`
  - compute `value_start = offset + size_of::<spa_pod_control>()`
  - check `value_start <= pod_size` and `value_size <= pod_size - value_start`
  - take `value` from the original byte slice/body window, not from a struct field pointer
  - use `checked_add` in `align_pod_size` or return `None` on overflow

- [ ] Add parser unit tests for:
  - non-sequence POD is ignored
  - truncated sequence body is ignored
  - truncated control is ignored
  - malformed oversized value is ignored
  - one valid MIDI control emits one `MidiCopy`

- [ ] Run `cargo test`.

- [ ] Run under Miri if available; otherwise document that Miri was unavailable because native PipeWire/JACK dependencies make it unsuitable here.

- [ ] Commit.

```bash
git add src/main.rs
git commit -m "fix: make pipewire midi sequence parsing bounds-safe"
```

## Task 5: Make Dropped MIDI Events Observable Off The RT Path

**Files:**
- Modify: `src/main.rs`

- [ ] Add an `Arc<AtomicU64>` counter for realtime queue drops and one for forwarder channel drops.

```rust
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
```

- [ ] Increment counters in callbacks instead of logging there.

```rust
if producer.try_push(event.into()).is_err() {
    rt_drops.fetch_add(1, Ordering::Relaxed);
}
```

- [ ] Spawn a non-RT monitor thread that wakes every 30 seconds and logs deltas only when nonzero.

- [ ] Add a unit test for a helper that computes drop deltas, if the code is not trivial.

- [ ] Run `cargo test`.

- [ ] Commit.

```bash
git add src/main.rs
git commit -m "feat: report dropped midi events off realtime threads"
```

## Task 6: Gate Runtime MIDI Logging Behind Verbose Mode

**Files:**
- Modify: `src/main.rs`
- Modify: `module.nix`
- Modify: `README.md`

- [ ] Add `--verbose` / `SPOTIFY_MIDI_VERBOSE` to `Args` and `Config`.

```rust
#[arg(long, env = "SPOTIFY_MIDI_VERBOSE")]
verbose: bool,
```

- [ ] Pass `verbose` into `spawn_controller`.

- [ ] Change unconditional `println!("midi data: {:?}", m);` to:

```rust
if verbose {
    println!("midi data: {:?}", m);
}
```

- [ ] Add `verbose` option to `module.nix` and append `--verbose` when true.

- [ ] Update `README.md` environment variable list.

- [ ] Run `cargo test` and `nix flake check --no-build`.

- [ ] Commit.

```bash
git add src/main.rs module.nix README.md
git commit -m "feat: gate midi debug logging behind verbose mode"
```

## Task 7: Align Nix MIDI Validation With Rust

**Files:**
- Modify: `module.nix`
- Modify: `flake.nix`

- [ ] Define a reusable MIDI command option type in `module.nix`.

```nix
midiCommandType = types.addCheck
  (types.nonEmptyListOf (types.ints.between 0 255))
  (command: length command <= 3);
```

- [ ] Use `midiCommandType` for play/pause/previous/next.

- [ ] Add or finish flake checks that prove:
  - `[ 176 41 127 ]` evaluates
  - `[ 176 41 127 1 ]` fails

- [ ] Run `nix flake check --no-build`.

- [ ] Commit.

```bash
git add module.nix flake.nix
git commit -m "fix: validate nix midi command lengths"
```

## Task 8: Add Contributor Verification That Matches Reality

**Files:**
- Modify: `flake.nix`
- Modify: `README.md`

- [ ] Add flake checks for formatting and tests if they are not already provided by package build.
  Prefer Nix-wrapped checks so PipeWire headers/toolchain are coherent:

```nix
cargo-test = pkgs.runCommand "spotify-midi-control-cargo-test" { } ''
  cp -r ${self} source
  chmod -R u+w source
  cd source
  ${pkgs.cargo}/bin/cargo test
  touch $out
'';
```

Adjust for `rustPlatform`/vendored dependencies if plain networkless Cargo cannot work in the Nix sandbox.

- [ ] Add a README contributor section:

```markdown
## Development checks

Use the flake checks for reproducible native headers and Rust tooling:

```sh
nix flake check
```

Inside the dev shell, stale `target/` artifacts from another Rust compiler can break Cargo. If that happens, run Cargo with a fresh target directory:

```sh
CARGO_TARGET_DIR=/tmp/spotify-midi-control-target cargo test
```
```
```

- [ ] If `cargo clippy` remains blocked by `libspa` header generation outside Nix, document Nix as the supported lint/build path rather than pretending direct Cargo is portable.

- [ ] Run `nix flake check --no-build` and `nix build --no-link`.

- [ ] Commit.

```bash
git add flake.nix README.md
git commit -m "chore: document reproducible development checks"
```

## Task 9: Final Verification

**Files:**
- No new edits unless verification exposes a failure.

- [ ] Run formatting.

```bash
cargo fmt --check
```

- [ ] Run Rust tests.

```bash
cargo test
```

- [ ] Run Clippy in a clean target dir if ambient dependencies allow it.

```bash
cargo clippy --all-targets --target-dir /tmp/spotify-midi-control-clippy-target -- -D warnings
```

- [ ] Run Nix checks and build.

```bash
nix flake check --no-build
nix build --no-link
```

- [ ] Run advisory tooling if available.

```bash
cargo audit
cargo deny check
```

If unavailable, record that the tools were not installed and leave the CI/Nix follow-up explicit.

- [ ] Commit any verification-only doc/check changes.

```bash
git status --short
```

Expected: only intentional changes remain.

## Risk Notes

- The PipeWire parser task is the riskiest. Keep that patch small and review every offset calculation.
- Do not log from JACK/PipeWire realtime callbacks.
- Do not remove `RT_PROCESS` or reintroduce blocking channels on callback threads.
- Treat existing `flake.nix` and `flake.lock` edits as user-owned unless this implementation explicitly needs to modify them.

## Self-Review

- Spec coverage: all review findings are covered by Tasks 2 through 8; Task 1 and Task 9 provide regression and final verification.
- Placeholder scan: no `TBD` or vague "handle errors" items remain; risky steps specify concrete behavior.
- Type consistency: `SpotifyAction` replaces string methods across bindings and DBus calls.
