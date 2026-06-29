use std::time::Duration;

use anyhow::{Context, Result};
use rumqttc::{AsyncClient, Event, MqttOptions, Packet, QoS};
use tracing::{error, info, warn};

struct Config {
    mqtt_hostname: String,
    mqtt_port: u16,
    mqtt_username: Option<String>,
    mqtt_password: Option<String>,
    retain: bool,
    max_volume: i64,
    volume_command_topic: String,
    shairplay_volume_topic: String,
}

fn env_opt(key: &str) -> Option<String> {
    std::env::var(key).ok().filter(|v| !v.is_empty())
}

fn env_or(key: &str, default: &str) -> String {
    env_opt(key).unwrap_or_else(|| default.to_string())
}

fn env_bool(key: &str, default: bool) -> bool {
    match env_opt(key) {
        Some(v) => matches!(v.to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "on"),
        None => default,
    }
}

impl Config {
    fn from_env() -> Result<Self> {
        Ok(Self {
            mqtt_hostname: env_or("MQTT_HOSTNAME", "127.0.0.1"),
            mqtt_port: env_or("MQTT_PORT", "1883")
                .parse()
                .context("MQTT_PORT must be a valid port number")?,
            mqtt_username: env_opt("MQTT_USERNAME"),
            mqtt_password: env_opt("MQTT_PASSWORD"),
            retain: env_bool("MQTT_RETAIN", false),
            max_volume: {
                let v: i64 = env_or("MAX_VOLUME", "32")
                    .parse()
                    .context("MAX_VOLUME must be an integer")?;
                if !(1..=32).contains(&v) {
                    anyhow::bail!("MAX_VOLUME must be between 1 and 32, got {v}");
                }
                v
            },
            volume_command_topic: env_or(
                "STANMORE2_VOLUME_COMMAND_TOPIC",
                "stanmore2/command/set_volume",
            ),
            shairplay_volume_topic: env_opt("SHAIRPLAY_VOLUME_TOPIC")
                .context("SHAIRPLAY_VOLUME_TOPIC environment variable must be set")?,
        })
    }
}

/// Map a shairport-sync AirPlay volume to the Marshall Stanmore II scale
/// (0..=`max_volume`).
///
/// AirPlay volume is in dB, running from 0.0 (loudest) down to -30.0, with
/// -144.0 meaning mute. `min_db` and below map to 0; 0.0 dB maps to
/// `max_volume`, letting the user cap the speaker's loudest output.
fn db_to_marshall_volume(db: f64, min_db: f64, max_volume: i64) -> i64 {
    if db <= min_db {
        return 0;
    }
    ((db + 30.0) * max_volume as f64 / 30.0) as i64
}

/// shairport-sync publishes the parsed volume as a comma-separated string whose
/// first field is the AirPlay volume in dB.
fn parse_volume(payload: &[u8], max_volume: i64) -> Option<i64> {
    let text = std::str::from_utf8(payload).ok()?;
    let db: f64 = text.split(',').next()?.trim().parse().ok()?;
    Some(db_to_marshall_volume(db, -30.0, max_volume))
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,rumqttc=warn".into()),
        )
        .init();

    let config = Config::from_env()?;

    let mut opts = MqttOptions::new(
        "shairplay-volume-sync",
        &config.mqtt_hostname,
        config.mqtt_port,
    );
    opts.set_keep_alive(Duration::from_secs(30));
    if let Some(user) = &config.mqtt_username {
        opts.set_credentials(user, config.mqtt_password.clone().unwrap_or_default());
    }

    let (client, mut eventloop) = AsyncClient::new(opts, 128);

    tokio::spawn(async move {
        wait_for_shutdown().await;
        warn!("Shutdown signal received");
        std::process::exit(0);
    });

    info!(
        source = %config.shairplay_volume_topic,
        target = %config.volume_command_topic,
        max_volume = config.max_volume,
        "Bridging shairport-sync volume to Stanmore II"
    );

    loop {
        match eventloop.poll().await {
            // rumqttc reconnects automatically but does not replay subscriptions,
            // so (re)subscribe every time a connection is (re)established.
            Ok(Event::Incoming(Packet::ConnAck(_))) => {
                info!("MQTT connected; subscribing to {}", config.shairplay_volume_topic);
                if let Err(e) = client
                    .subscribe(&config.shairplay_volume_topic, QoS::AtLeastOnce)
                    .await
                {
                    error!("failed to subscribe: {e}");
                }
            }
            Ok(Event::Incoming(Packet::Publish(p))) => {
                let body = String::from_utf8_lossy(&p.payload);
                info!(topic = %p.topic, payload = %body, "Volume message");
                match parse_volume(&p.payload, config.max_volume) {
                    Some(volume) => {
                        info!(topic = %config.volume_command_topic, volume, "Publishing volume");
                        if let Err(e) = client
                            .publish(
                                &config.volume_command_topic,
                                QoS::AtLeastOnce,
                                config.retain,
                                volume.to_string(),
                            )
                            .await
                        {
                            error!("failed to publish volume: {e}");
                        }
                    }
                    None => warn!(payload = %body, "Could not parse volume from message"),
                }
            }
            Ok(_) => {}
            Err(e) => {
                error!("MQTT connection error: {e}; retrying in 5s");
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
        }
    }
}

#[cfg(unix)]
async fn wait_for_shutdown() {
    use tokio::signal::unix::{signal, SignalKind};
    let mut term = signal(SignalKind::terminate()).expect("install SIGTERM handler");
    let mut int = signal(SignalKind::interrupt()).expect("install SIGINT handler");
    tokio::select! {
        _ = term.recv() => {}
        _ = int.recv() => {}
    }
}

#[cfg(not(unix))]
async fn wait_for_shutdown() {
    let _ = tokio::signal::ctrl_c().await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_full_range() {
        assert_eq!(db_to_marshall_volume(0.0, -30.0, 32), 32);
        assert_eq!(db_to_marshall_volume(-30.0, -30.0, 32), 0);
        assert_eq!(db_to_marshall_volume(-15.0, -30.0, 32), 16);
        assert_eq!(db_to_marshall_volume(-22.5, -30.0, 32), 8);
    }

    #[test]
    fn scales_to_max_volume() {
        assert_eq!(db_to_marshall_volume(0.0, -30.0, 10), 10);
        assert_eq!(db_to_marshall_volume(-15.0, -30.0, 10), 5);
        assert_eq!(db_to_marshall_volume(-30.0, -30.0, 10), 0);
        assert_eq!(db_to_marshall_volume(0.0, -30.0, 1), 1);
    }

    #[test]
    fn mute_and_below_min_clamp_to_zero() {
        assert_eq!(db_to_marshall_volume(-144.0, -30.0, 32), 0);
        assert_eq!(db_to_marshall_volume(-100.0, -30.0, 32), 0);
    }

    #[test]
    fn parses_comma_separated_payload() {
        assert_eq!(parse_volume(b"0.0,0.00,-30.00,0.00", 32), Some(32));
        assert_eq!(parse_volume(b"-15.0", 32), Some(16));
        assert_eq!(parse_volume(b"-144.0,...", 32), Some(0));
        assert_eq!(parse_volume(b"not-a-number", 32), None);
    }
}
