{
  description = "shairport-sync -> Marshall Stanmore II MQTT volume bridge";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    crane.url = "github:ipetkov/crane";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, crane, flake-utils }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs { inherit system; };
        craneLib = crane.mkLib pkgs;

        commonArgs = {
          src = craneLib.cleanCargoSource ./.;
          strictDeps = true;
        };

        cargoArtifacts = craneLib.buildDepsOnly commonArgs;

        volume-sync = craneLib.buildPackage (commonArgs // {
          inherit cargoArtifacts;
          meta.mainProgram = "volume-sync";
        });
      in
      {
        packages = {
          default = volume-sync;
          volume-sync = volume-sync;
        };

        checks.volume-sync = volume-sync;

        devShells.default = craneLib.devShell { };
      })
    // {
      # NixOS service module. Import on the RPi 3 config and set
      # `services.volume-sync.shairplayVolumeTopic`.
      nixosModules.default = { config, lib, pkgs, ... }:
        let
          cfg = config.services.volume-sync;
        in
        {
          options.services.volume-sync = {
            enable = lib.mkEnableOption "shairport-sync -> Stanmore II MQTT volume bridge";

            package = lib.mkOption {
              type = lib.types.package;
              default = self.packages.${pkgs.stdenv.hostPlatform.system}.default;
              description = "volume-sync package to run.";
            };

            shairplayVolumeTopic = lib.mkOption {
              type = lib.types.str;
              example = "marshall/volume";
              description = "MQTT topic shairport-sync publishes volume on (SHAIRPLAY_VOLUME_TOPIC).";
            };

            environment = lib.mkOption {
              type = lib.types.attrsOf lib.types.str;
              default = { };
              example = {
                MQTT_HOSTNAME = "192.168.1.10";
                STANMORE2_VOLUME_COMMAND_TOPIC = "stanmore2/command/set_volume";
                MQTT_RETAIN = "1";
                MAX_VOLUME = "24";
              };
              description = "Extra environment variables (MQTT_*, STANMORE2_VOLUME_COMMAND_TOPIC, MAX_VOLUME).";
            };

            environmentFile = lib.mkOption {
              type = lib.types.nullOr lib.types.path;
              default = null;
              description = "File with secrets like MQTT_PASSWORD (systemd EnvironmentFile).";
            };
          };

          config = lib.mkIf cfg.enable {
            systemd.services.volume-sync = {
              description = "shairport-sync -> Stanmore II MQTT volume bridge";
              wantedBy = [ "multi-user.target" ];
              wants = [ "network-online.target" ];
              after = [ "network-online.target" ];

              environment =
                { SHAIRPLAY_VOLUME_TOPIC = cfg.shairplayVolumeTopic; } // cfg.environment;

              serviceConfig = {
                ExecStart = lib.getExe cfg.package;
                Restart = "on-failure";
                RestartSec = 5;
                # No hardware access needed — run as an ephemeral unprivileged user.
                DynamicUser = true;
                EnvironmentFile = lib.mkIf (cfg.environmentFile != null) cfg.environmentFile;
              };
            };
          };
        };
    };
}
