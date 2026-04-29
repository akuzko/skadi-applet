// SPDX-License-Identifier: MIT

//! MPRIS state subscription (1-second poll).
//!
//! Heuristic for active player:
//!   - If a player is currently `Playing`, it becomes the tracked player.
//!   - If the previously tracked player is still available (regardless of
//!     status), keep it.
//!   - Otherwise fall back to the first player found.

use cosmic::iced::Subscription;
use cosmic::iced::stream;
use futures::channel::mpsc::Sender;
use futures_util::SinkExt;
use tokio::time::{Duration, interval};
use tracing::warn;

use crate::backend::mpris::{get_playback_status, list_players};
use crate::types::MprisPlayer;

#[derive(Clone, Debug)]
pub struct MprisState {
    pub active_player: Option<MprisPlayer>,
    pub is_playing: bool,
}

pub fn mpris_subscription() -> Subscription<MprisState> {
    Subscription::run_with(std::any::TypeId::of::<MprisSubscriptionId>(), |_| {
        stream::channel(16, |mut tx: Sender<MprisState>| async move {
            // Establish a persistent session-bus connection
            let conn = match zbus::Connection::session().await {
                Ok(c) => c,
                Err(e) => {
                    warn!("MPRIS: failed to connect to session bus: {e}");
                    // Emit empty state and exit; subscription will be inactive
                    let _ = tx
                        .send(MprisState {
                            active_player: None,
                            is_playing: false,
                        })
                        .await;
                    return;
                }
            };

            let mut last_active: Option<MprisPlayer> = None;
            let mut ticker = interval(Duration::from_secs(1));

            loop {
                ticker.tick().await;

                let state = poll_state(&conn, &mut last_active).await;
                let _ = tx.send(state).await;
            }
        })
    })
}

struct MprisSubscriptionId;

async fn poll_state(
    conn: &zbus::Connection,
    last_active: &mut Option<MprisPlayer>,
) -> MprisState {
    let players = match list_players(conn).await {
        Ok(p) => p,
        Err(e) => {
            warn!("MPRIS list_players failed: {e}");
            return MprisState {
                active_player: None,
                is_playing: false,
            };
        }
    };

    if players.is_empty() {
        *last_active = None;
        return MprisState {
            active_player: None,
            is_playing: false,
        };
    }

    // Gather statuses
    let mut statuses: Vec<(MprisPlayer, String)> = Vec::new();
    for player in &players {
        let status = get_playback_status(conn, &player.bus_name)
            .await
            .unwrap_or_else(|e| {
                warn!("MPRIS get_playback_status for {} failed: {e}", player.name);
                "Stopped".to_string()
            });
        statuses.push((player.clone(), status));
    }

    // 1. A currently-Playing player always wins
    if let Some((player, _)) = statuses.iter().find(|(_, s)| s == "Playing") {
        let is_playing = true;
        *last_active = Some(player.clone());
        return MprisState {
            active_player: Some(player.clone()),
            is_playing,
        };
    }

    // 2. If the previously-tracked player is still present, keep it
    if let Some(ref prev) = last_active.clone() {
        if let Some((player, status)) = statuses.iter().find(|(p, _)| p.bus_name == prev.bus_name) {
            return MprisState {
                active_player: Some(player.clone()),
                is_playing: status == "Playing",
            };
        }
    }

    // 3. Fall back to first available player
    let (player, status) = &statuses[0];
    *last_active = Some(player.clone());
    MprisState {
        active_player: Some(player.clone()),
        is_playing: status == "Playing",
    }
}
