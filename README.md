# shairplay-stanmore-2-volume-sync (Rust)

A Rust rewrite of the Python shairport-sync → Marshall Stanmore II volume bridge.

shairport-sync runs with `ignore_volume_control = "yes"`, so its audio output
always plays at 100% (no double attenuation). Instead of attenuating the audio,
this daemon listens for the AirPlay volume that shairport-sync publishes over
MQTT and forwards it as a hardware volume command to the speaker via the
`stanmore2` BLE↔MQTT bridge. The result: the AirPlay volume slider drives the
speaker's own hardware volume.

```
shairport-sync ──(MQTT: SHAIRPLAY_VOLUME_TOPIC, dB)──▶ volume-sync
volume-sync ──(MQTT: STANMORE2_VOLUME_COMMAND_TOPIC, 0..32)──▶ stanmore2 ──BLE──▶ speaker
```

Binary name is `volume-sync` (crate is `shairplay-stanmore-2-volume-sync`).

## How the conversion works

shairport-sync's `publish_parsed` volume payload is a comma-separated string
whose first field is the AirPlay volume in dB. AirPlay volume runs from `0.0`
(loudest) down to `-30.0`, with `-144.0` meaning mute. This maps linearly onto
the Stanmore II's `0..=MAX_VOLUME` hardware scale (`MAX_VOLUME` defaults to 32,
the speaker's maximum):

```
volume = (db + 30) * MAX_VOLUME / 30   (db <= -30  → 0,   db = 0 → MAX_VOLUME)
```

Set `MAX_VOLUME` below 32 to cap how loud the speaker can get even at full
AirPlay volume.

## Configuration

All configuration is via environment variables (names match the Python version
for drop-in compatibility):

| Variable                        | Default                        | Description                                              |
|---------------------------------|--------------------------------|----------------------------------------------------------|
| `SHAIRPLAY_VOLUME_TOPIC`        | *(required)*                   | MQTT topic shairport-sync publishes volume on, e.g. `marshall/volume` |
| `STANMORE2_VOLUME_COMMAND_TOPIC`| `stanmore2/command/set_volume` | MQTT topic the speaker bridge listens on for volume      |
| `MQTT_HOSTNAME`                 | `127.0.0.1`                    | MQTT broker host                                         |
| `MQTT_PORT`                     | `1883`                         | MQTT broker port                                         |
| `MQTT_USERNAME`                 | *(unset)*                      | MQTT username                                            |
| `MQTT_PASSWORD`                 | *(unset)*                      | MQTT password                                            |
| `MQTT_RETAIN`                   | `0`                            | Retain flag on published volume commands (`1`/`0`)       |
| `MAX_VOLUME`                    | `32`                           | Cap for the speaker volume scale, `1`–`32`. Full AirPlay volume maps to this |

`RUST_LOG` controls log verbosity (e.g. `RUST_LOG=debug`).

> The matching shairport-sync config sets `topic = "marshall"` and
> `publish_parsed = "yes"`, which publishes the volume on `marshall/volume`.

## Running

```bash
SHAIRPLAY_VOLUME_TOPIC=marshall/volume MQTT_HOSTNAME=192.168.1.10 cargo run --release
```

Or with Docker (host networking to reach the broker):

```bash
SHAIRPLAY_VOLUME_TOPIC=marshall/volume docker compose up --build
```

## Deploying to NixOS (Raspberry Pi 3)

This repo is a flake exposing `packages.<system>.default` and
`nixosModules.default`. Reference it from the Pi's flake config:

```nix
{
  inputs.volume-sync.url = "github:you/shairplay-stanmore-2-volume-sync-rust";

  outputs = { self, nixpkgs, volume-sync, ... }: {
    nixosConfigurations.marshall = nixpkgs.lib.nixosSystem {
      system = "aarch64-linux";
      modules = [
        volume-sync.nixosModules.default
        {
          services.volume-sync = {
            enable = true;
            shairplayVolumeTopic = "marshall/volume";
            environment = {
              MQTT_HOSTNAME = "192.168.1.10";
              MQTT_RETAIN = "1";
            };
            # Keep secrets out of the store:
            environmentFile = "/etc/nix-env/volume-sync.env"; # MQTT_PASSWORD=...
          };
        }
      ];
    };
  };
}
```

Build on a stronger machine and push the closure to the Pi (the Pi compiles
nothing):

```bash
nixos-rebuild switch \
  --flake .#marshall \
  --target-host root@pi \
  --build-host localhost \
  --use-remote-sudo
```

### Local builds

```bash
nix build .#volume-sync   # result/bin/volume-sync
nix develop               # dev shell
nix flake check
```

## Prebuilt binary releases (Raspberry Pi 3 / aarch64)

The `Release` GitHub Action (`.github/workflows/release.yml`) builds a
**static `aarch64-unknown-linux-musl` binary** natively on GitHub's arm64
hosted runners and attaches it (as a `.tar.gz` plus `.sha256`) to the GitHub
Release for the pushed tag. Being static musl, it runs on the Pi 3 regardless
of the host distro (incl. NixOS).

> The arm64 hosted runners are free on public repos; on private repos they are
> a paid (larger-runner) feature.

Cut a release by pushing a tag:

```bash
git tag v0.1.0
git push origin v0.1.0
```

The workflow builds and uploads to the release page automatically. You can also
run it manually from the **Actions** tab against an existing tag. On the Pi:

```bash
tar xzf volume-sync-v0.1.0-aarch64-unknown-linux-musl.tar.gz
./volume-sync   # with the env vars above set
```

## Behavior notes

- **Auto-reconnect, in-process.** rumqttc reconnects on connection loss; the
  bridge re-subscribes on every `ConnAck`. On unrecoverable errors it backs off
  5s and retries. The supervisor (systemd `Restart=on-failure` / docker
  `restart: unless-stopped`) covers process-level failures.
- The volume command topic and payload (`0..32` integer) match what the
  `stanmore2` bridge expects on `…/command/set_volume`.
