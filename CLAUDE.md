# CLAUDE.md

shairport-sync → Marshall Stanmore II MQTT volume bridge. Rust rewrite of a
Python project. Subscribes to the AirPlay volume shairport-sync publishes over
MQTT (dB), maps it to the speaker's `0..32` hardware scale, and republishes it
to the `stanmore2` bridge's `set_volume` command topic.

shairport-sync runs with `ignore_volume_control = "yes"` (audio always at 100%);
this bridge drives the speaker's *hardware* volume instead, so the AirPlay
slider controls the speaker directly without double attenuation.

Binary name is `volume-sync` (crate is `shairplay-stanmore-2-volume-sync`).

## Build & run

```bash
cargo build
cargo test                       # db→volume conversion tests
SHAIRPLAY_VOLUME_TOPIC=marshall/volume MQTT_HOSTNAME=192.168.1.10 cargo run --release

nix build .#volume-sync          # -> result/bin/volume-sync
nix develop
nix flake check
```

All config is via env vars — see README.md. `SHAIRPLAY_VOLUME_TOPIC` is
required. Env var names match the Python version for drop-in compatibility.

## Architecture

- `src/main.rs` — the whole program. `Config::from_env` loads MQTT + topic
  settings. `db_to_marshall_volume` does the dB→`0..MAX_VOLUME` mapping
  (`MAX_VOLUME` 1–32, default 32, caps the speaker's loudest output);
  `parse_volume` extracts the first comma-separated field of the shairport
  payload and converts it. `main` runs the rumqttc event loop: re-subscribe on
  `ConnAck`, convert+republish on each `Publish`.

## Behavior notes

- **No BLE here.** Unlike the sibling `stanmore2` bridge, this is pure MQTT —
  no btleplug/dbus, so the flake/Docker need no `pkg-config`/`dbus`.
- rumqttc reconnects automatically but does **not** replay subscriptions, so the
  loop re-subscribes on every `ConnAck`.
- Topic/payload contract: publishes an integer `0..32` to
  `STANMORE2_VOLUME_COMMAND_TOPIC` (default `stanmore2/command/set_volume`),
  which the `stanmore2` bridge consumes unchanged.

## Deployment

Structured exactly like the sibling `marshall-stanmore-2-rust`: a flake exposing
`packages.<system>.default` + `nixosModules.default` (`services.volume-sync`),
plus a Dockerfile/compose for local runs. The NixOS module runs the service
under `DynamicUser` (no hardware access needed). The Pi compiles nothing — build
on a stronger machine and push the closure.

## Conventions

- No unused/dead code; keep it building warning-clean.
- Env var names are kept identical to the Python version on purpose.
