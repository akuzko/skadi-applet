// SPDX-License-Identifier: MIT

/// Represents an audio output sink (device).
#[derive(Clone, Debug, PartialEq)]
pub struct AudioDevice {
    pub id: u32,
    pub name: String,
}

/// Represents an MPRIS-capable media player.
#[derive(Clone, Debug, PartialEq)]
pub struct MprisPlayer {
    /// The D-Bus well-known name, e.g. `org.mpris.MediaPlayer2.rhythmbox`.
    pub bus_name: String,
    /// Human-readable player name from the `Identity` property.
    pub name: String,
}
