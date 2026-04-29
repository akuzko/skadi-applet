// SPDX-License-Identifier: MIT

//! Audio state subscription.
//!
//! Strategy:
//!   1. Try to subscribe to PipeWire's PulseAudio-compat D-Bus signals for
//!      reactive updates. PipeWire exposes `org.PulseAudio.Core1` on the session
//!      bus when the PA compatibility module is loaded.
//!   2. If the D-Bus service is unavailable, fall back to polling every second.
//!
//! In both cases the actual state is read via `wpctl` CLI.

use cosmic::iced::Subscription;
use cosmic::iced::stream;
use futures::channel::mpsc::Sender;
use futures_util::SinkExt;
use tokio::time::{Duration, interval};
use tracing::warn;

use crate::backend::audio::{AudioState, fetch_audio_state};

pub fn audio_subscription() -> Subscription<AudioState> {
    Subscription::run_with(std::any::TypeId::of::<AudioSubscriptionId>(), |_| {
        stream::channel(16, |mut tx: Sender<AudioState>| async move {
            // Send initial state immediately
            match fetch_audio_state().await {
                Ok(state) => {
                    let _ = tx.send(state).await;
                }
                Err(e) => warn!("initial audio state fetch failed: {e}"),
            }

            // Try event-driven via D-Bus
            let conn = zbus::Connection::session().await;
            match conn {
                Err(e) => {
                    warn!("audio: D-Bus session connection failed ({e}), using polling");
                    run_polling(&mut tx).await;
                }
                Ok(conn) => {
                    if let Err(e) = run_dbus_driven(&conn, &mut tx).await {
                        warn!("audio: D-Bus subscription failed ({e}), falling back to polling");
                        run_polling(&mut tx).await;
                    }
                }
            }
        })
    })
}

struct AudioSubscriptionId;

/// Attempt to subscribe to PulseAudio-compat signals from PipeWire.
async fn run_dbus_driven<S>(conn: &zbus::Connection, tx: &mut S) -> anyhow::Result<()>
where
    S: futures_util::Sink<AudioState> + Unpin,
    S::Error: std::fmt::Debug,
{
    use zbus::fdo::DBusProxy;

    // Check whether the PipeWire PA compat service is on the bus
    let dbus = DBusProxy::new(conn).await?;
    let names = dbus.list_names().await?;
    let has_pa_compat = names.iter().any(|n| n.as_str() == "org.PulseAudio1");

    if !has_pa_compat {
        anyhow::bail!("org.PulseAudio1 not available on session bus");
    }

    // Subscribe to property-change signals from the PA compat service
    let match_rule = zbus::MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .sender("org.PulseAudio1")
        .unwrap()
        .interface("org.freedesktop.DBus.Properties")
        .unwrap()
        .member("PropertiesChanged")
        .unwrap()
        .build();

    let mut stream =
        zbus::MessageStream::for_match_rule(match_rule, conn, Some(64)).await?;

    use futures_util::StreamExt;
    while stream.next().await.is_some() {
        match fetch_audio_state().await {
            Ok(state) => {
                let _ = tx.send(state).await;
            }
            Err(e) => warn!("audio state fetch failed: {e}"),
        }
    }

    Ok(())
}

/// Polling fallback: refresh state every second.
async fn run_polling<S>(tx: &mut S)
where
    S: futures_util::Sink<AudioState> + Unpin,
    S::Error: std::fmt::Debug,
{
    let mut ticker = interval(Duration::from_secs(1));
    loop {
        ticker.tick().await;
        match fetch_audio_state().await {
            Ok(state) => {
                let _ = tx.send(state).await;
            }
            Err(e) => warn!("audio state fetch failed: {e}"),
        }
    }
}
