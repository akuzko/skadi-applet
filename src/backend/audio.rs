// SPDX-License-Identifier: MIT

use anyhow::{Context, Result, bail};
use tokio::process::Command;
use tracing::warn;

use crate::types::AudioDevice;

/// The full audio state retrieved in a single pass.
#[derive(Clone, Debug)]
pub struct AudioState {
    pub is_muted: bool,
    pub devices: Vec<AudioDevice>,
    pub default_device_id: Option<u32>,
}

/// Retrieve mute status via `wpctl get-volume @DEFAULT_AUDIO_SINK@`.
/// The output looks like:
///   Volume: 0.65
///   Volume: 0.65 [MUTED]
pub async fn get_mute_status() -> Result<bool> {
    let out = Command::new("wpctl")
        .args(["get-volume", "@DEFAULT_AUDIO_SINK@"])
        .output()
        .await
        .context("failed to run wpctl get-volume")?;

    if !out.status.success() {
        bail!(
            "wpctl get-volume exited with {}: {}",
            out.status,
            String::from_utf8_lossy(&out.stderr)
        );
    }

    let stdout = String::from_utf8_lossy(&out.stdout);
    Ok(stdout.contains("[MUTED]"))
}

/// Toggle mute on the default sink.
pub async fn toggle_mute() -> Result<()> {
    let status = Command::new("wpctl")
        .args(["set-mute", "@DEFAULT_AUDIO_SINK@", "toggle"])
        .status()
        .await
        .context("failed to run wpctl set-mute")?;

    if !status.success() {
        bail!("wpctl set-mute exited with {status}");
    }
    Ok(())
}

/// Parse `wpctl status` output and extract sinks.
///
/// Relevant section looks like:
/// ```
///  Audio
///   ├─ Sinks:
///   │      51. Built-in Audio Analog Stereo        [vol: 0.65]
///   │  *   52. HDMI / DisplayPort                  [vol: 1.00]
/// ```
/// The `*` marks the default sink.
pub async fn list_sinks_and_default() -> Result<(Vec<AudioDevice>, Option<u32>)> {
    let out = Command::new("wpctl")
        .arg("status")
        .output()
        .await
        .context("failed to run wpctl status")?;

    if !out.status.success() {
        bail!(
            "wpctl status exited with {}: {}",
            out.status,
            String::from_utf8_lossy(&out.stderr)
        );
    }

    let stdout = String::from_utf8_lossy(&out.stdout);
    parse_sinks(&stdout)
}

fn parse_sinks(status: &str) -> Result<(Vec<AudioDevice>, Option<u32>)> {
    let mut in_audio = false;
    let mut in_sinks = false;
    let mut devices = Vec::new();
    let mut default_id: Option<u32> = None;

    for line in status.lines() {
        // Detect the Audio section
        if line.trim_start().starts_with("Audio") && !line.contains("MIDI") {
            in_audio = true;
            continue;
        }

        if !in_audio {
            continue;
        }

        // Detect the Sinks subsection header
        if line.contains("Sinks:") {
            in_sinks = true;
            continue;
        }

        // Once we hit another subsection or leave Audio, stop
        if in_sinks {
            // Subsection headers contain a word followed by ':'
            // but sink lines contain numbers; stop on next subsection
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            // Another subsection detected (e.g. "Sources:", "Filters:", or a new section)
            if trimmed.ends_with(':') || (!trimmed.starts_with('*') && !trimmed.chars().next().map(|c| c.is_ascii_digit()).unwrap_or(false) && !trimmed.starts_with('│') && !trimmed.starts_with("├") && !trimmed.starts_with("└") && !trimmed.starts_with('|')) {
                break;
            }
        }

        if !in_sinks {
            continue;
        }

        // Parse a sink line. Examples:
        //   │      51. Built-in Audio Analog Stereo      [vol: 0.65]
        //   │  *   52. HDMI / DisplayPort                [vol: 1.00]
        if let Some((id, name, is_default)) = parse_sink_line(line) {
            if is_default {
                default_id = Some(id);
            }
            devices.push(AudioDevice { id, name });
        }
    }

    Ok((devices, default_id))
}

/// Parse a single sink line, returning (id, name, is_default).
fn parse_sink_line(line: &str) -> Option<(u32, String, bool)> {
    // Strip tree-drawing characters and leading whitespace
    let stripped: String = line
        .chars()
        .filter(|c| !matches!(c, '│' | '├' | '└' | '─'))
        .collect();
    let stripped = stripped.trim();

    let is_default = stripped.starts_with('*');
    let stripped = stripped.trim_start_matches('*').trim();

    // Now expect "ID. Name   [vol: x.xx]"
    let dot_pos = stripped.find('.')?;
    let id_str = stripped[..dot_pos].trim();
    let id: u32 = id_str.parse().ok()?;

    let rest = stripped[dot_pos + 1..].trim();
    // Strip trailing metadata like [vol: ...] or [INACTIVE]
    let name = if let Some(bracket_pos) = rest.rfind('[') {
        rest[..bracket_pos].trim().to_string()
    } else {
        rest.to_string()
    };

    if name.is_empty() {
        return None;
    }

    Some((id, name, is_default))
}

/// Set the default audio sink by id.
pub async fn set_default_sink(id: u32) -> Result<()> {
    let status = Command::new("wpctl")
        .args(["set-default", &id.to_string()])
        .status()
        .await
        .context("failed to run wpctl set-default")?;

    if !status.success() {
        bail!("wpctl set-default {id} exited with {status}");
    }
    Ok(())
}

/// Fetch the complete audio state in one call (mute + sinks).
pub async fn fetch_audio_state() -> Result<AudioState> {
    // Run both in parallel
    let (mute_result, sinks_result) =
        tokio::join!(get_mute_status(), list_sinks_and_default());

    let is_muted = mute_result.unwrap_or_else(|e| {
        warn!("failed to get mute status: {e}");
        false
    });

    let (devices, default_device_id) = sinks_result.unwrap_or_else(|e| {
        warn!("failed to list sinks: {e}");
        (Vec::new(), None)
    });

    Ok(AudioState {
        is_muted,
        devices,
        default_device_id,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_STATUS: &str = r#"
PipeWire 'pipewire-0' [v1.0.3, artem@darter, fd=17]

 Devices:
    43. Built-in Audio                           [alsa]

 Audio
  ├─ Sinks:
  │      51. Built-in Audio Analog Stereo        [vol: 0.65]
  │  *   52. HDMI / DisplayPort                  [vol: 1.00]
  ├─ Sources:
  │      53. Built-in Audio Analog Stereo        [vol: 1.00]
  └─ Filters:
"#;

    #[test]
    fn test_parse_sinks() {
        let (devices, default_id) = parse_sinks(SAMPLE_STATUS).unwrap();
        assert_eq!(devices.len(), 2);
        assert_eq!(devices[0].id, 51);
        assert_eq!(devices[0].name, "Built-in Audio Analog Stereo");
        assert_eq!(devices[1].id, 52);
        assert_eq!(devices[1].name, "HDMI / DisplayPort");
        assert_eq!(default_id, Some(52));
    }
}
