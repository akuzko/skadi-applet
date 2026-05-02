// SPDX-License-Identifier: MIT

use std::collections::HashMap;
use std::ops::Deref;

use anyhow::{Context, Result};
use zbus::Connection;
use zbus::fdo::DBusProxy;
use zbus::names::BusName;
use zbus::proxy;
use zbus::zvariant::{OwnedValue, Value};

use crate::types::{MprisPlayer, TrackInfo};

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
    fn next(&self) -> zbus::Result<()>;
    fn previous(&self) -> zbus::Result<()>;

    #[zbus(property)]
    fn playback_status(&self) -> zbus::Result<String>;

    #[zbus(property)]
    fn metadata(&self) -> zbus::Result<HashMap<String, OwnedValue>>;

    #[zbus(property)]
    fn can_go_next(&self) -> zbus::Result<bool>;

    #[zbus(property)]
    fn can_go_previous(&self) -> zbus::Result<bool>;
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

/// Fetch track metadata (`xesam:title`, `xesam:artist`) for a player.
/// Returns `TrackInfo` with whichever fields could be read; all fields are
/// `None` if metadata is unavailable.
pub async fn get_track_info(conn: &Connection, bus_name: &str) -> TrackInfo {
    let Ok(bus) = BusName::try_from(bus_name) else {
        return TrackInfo::default();
    };

    let proxy = match MediaPlayer2PlayerProxy::builder(conn)
        .destination(bus)
        .unwrap()
        .build()
        .await
    {
        Ok(p) => p,
        Err(_) => return TrackInfo::default(),
    };

    let metadata = match proxy.metadata().await {
        Ok(m) => m,
        Err(_) => return TrackInfo::default(),
    };

    let title = metadata
        .get("xesam:title")
        .and_then(|v| match v.deref() {
            Value::Str(s) => Some(s.to_string()),
            _ => None,
        })
        .filter(|s| !s.is_empty());

    let artist = metadata
        .get("xesam:artist")
        .and_then(|v| match v.deref() {
            Value::Array(arr) => {
                let strings: Vec<String> = arr
                    .iter()
                    .filter_map(|item| {
                        if let Value::Str(s) = item {
                            Some(s.to_string())
                        } else {
                            None
                        }
                    })
                    .collect();
                if strings.is_empty() { None } else { Some(strings.join(", ")) }
            }
            _ => None,
        })
        .filter(|s| !s.is_empty());

    TrackInfo { title, artist }
}

/// Send `Next` to a player.
pub async fn next_track(conn: &Connection, bus_name: &str) -> Result<()> {
    let bus: BusName<'_> = BusName::try_from(bus_name)
        .context("invalid D-Bus bus name")?;

    let proxy = MediaPlayer2PlayerProxy::builder(conn)
        .destination(bus)
        .unwrap()
        .build()
        .await
        .context("failed to build MediaPlayer2Player proxy")?;

    proxy.next().await.context("Next call failed")
}

/// Send `Previous` to a player.
pub async fn previous_track(conn: &Connection, bus_name: &str) -> Result<()> {
    let bus: BusName<'_> = BusName::try_from(bus_name)
        .context("invalid D-Bus bus name")?;

    let proxy = MediaPlayer2PlayerProxy::builder(conn)
        .destination(bus)
        .unwrap()
        .build()
        .await
        .context("failed to build MediaPlayer2Player proxy")?;

    proxy.previous().await.context("Previous call failed")
}

/// Query whether the player supports `Next` and `Previous`.
/// Returns `(can_go_previous, can_go_next)`.
pub async fn get_nav_caps(conn: &Connection, bus_name: &str) -> (bool, bool) {
    let Ok(bus) = BusName::try_from(bus_name) else {
        return (false, false);
    };

    let proxy = match MediaPlayer2PlayerProxy::builder(conn)
        .destination(bus)
        .unwrap()
        .build()
        .await
    {
        Ok(p) => p,
        Err(_) => return (false, false),
    };

    let can_prev = proxy.can_go_previous().await.unwrap_or(false);
    let can_next = proxy.can_go_next().await.unwrap_or(false);
    (can_prev, can_next)
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
