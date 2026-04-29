// SPDX-License-Identifier: MIT

use anyhow::{Context, Result};
use zbus::Connection;
use zbus::fdo::DBusProxy;
use zbus::names::BusName;
use zbus::proxy;

use crate::types::MprisPlayer;

const MPRIS_PREFIX: &str = "org.mpris.MediaPlayer2.";

// ── zbus proxy definitions ────────────────────────────────────────────────────

#[proxy(
    interface = "org.mpris.MediaPlayer2",
    default_path = "/org/mpris/MediaPlayer2"
)]
trait MediaPlayer2 {
    #[zbus(property)]
    fn identity(&self) -> zbus::Result<String>;
}

#[proxy(
    interface = "org.mpris.MediaPlayer2.Player",
    default_path = "/org/mpris/MediaPlayer2"
)]
trait MediaPlayer2Player {
    fn play_pause(&self) -> zbus::Result<()>;

    #[zbus(property)]
    fn playback_status(&self) -> zbus::Result<String>;
}

// ── Public API ────────────────────────────────────────────────────────────────

/// List all currently running MPRIS players.
pub async fn list_players(conn: &Connection) -> Result<Vec<MprisPlayer>> {
    let dbus = DBusProxy::new(conn)
        .await
        .context("failed to connect to org.freedesktop.DBus")?;

    let names = dbus
        .list_names()
        .await
        .context("failed to list D-Bus names")?;

    let mut players = Vec::new();

    for name in names {
        let name_str = name.as_str();
        if !name_str.starts_with(MPRIS_PREFIX) {
            continue;
        }

        // Try to read the human-readable Identity property
        let bus_name: BusName<'_> = match BusName::try_from(name_str) {
            Ok(b) => b,
            Err(_) => continue,
        };

        let identity = match MediaPlayer2Proxy::builder(conn)
            .destination(bus_name.clone())
            .unwrap()
            .build()
            .await
        {
            Ok(proxy) => proxy.identity().await.unwrap_or_else(|_| name_str.to_string()),
            Err(_) => name_str.to_string(),
        };

        players.push(MprisPlayer {
            bus_name: name_str.to_string(),
            name: identity,
        });
    }

    Ok(players)
}

/// Get the `PlaybackStatus` string for a player (`"Playing"`, `"Paused"`, `"Stopped"`).
pub async fn get_playback_status(conn: &Connection, bus_name: &str) -> Result<String> {
    let bus: BusName<'_> = BusName::try_from(bus_name)
        .context("invalid D-Bus bus name")?;

    let proxy = MediaPlayer2PlayerProxy::builder(conn)
        .destination(bus)
        .unwrap()
        .build()
        .await
        .context("failed to build MediaPlayer2Player proxy")?;

    proxy
        .playback_status()
        .await
        .context("failed to get PlaybackStatus")
}

/// Send `PlayPause` to a player.
pub async fn play_pause(conn: &Connection, bus_name: &str) -> Result<()> {
    let bus: BusName<'_> = BusName::try_from(bus_name)
        .context("invalid D-Bus bus name")?;

    let proxy = MediaPlayer2PlayerProxy::builder(conn)
        .destination(bus)
        .unwrap()
        .build()
        .await
        .context("failed to build MediaPlayer2Player proxy")?;

    proxy.play_pause().await.context("PlayPause call failed")
}
