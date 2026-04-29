# Copilot Instructions — skadi-applet

## Project Overview

A COSMIC desktop panel applet written in Rust that provides:

- Audio mute/unmute toggle and output device switching (via wpctl CLI / PipeWire)
- MPRIS media player play/pause control (via D-Bus / zbus)

App ID: `io.github.akuzko.skadi-applet`

## Toolchain & Runtime

- **Rust edition 2024**, stable toolchain managed via **asdf** (`.tool-versions`)
- **libcosmic** as a git dependency from `https://github.com/pop-os/libcosmic.git`
  - Features: `applet`, `applet-token`, `dbus-config`, `multi-window`, `tokio`, `wayland`, `winit`
- **tokio** `"full"` for async runtime
- **zbus 4** (tokio feature, `default-features = false`) for D-Bus
- **futures 0.3** for `futures::channel::mpsc::Sender<T>` (required by `iced::stream::channel`)
- **anyhow 1** for error handling in backend code
- **tracing** + **tracing-subscriber** for logging
- Build/install recipes defined in `justfile`

## Module Structure

```
src/
  main.rs           — entry point; tracing init; calls cosmic::applet::run::<app::AppModel>(())
  app.rs            — AppModel, Message enum, Application trait impl, all UI
  types.rs          — AudioDevice { id: u32, name: String }, MprisPlayer { bus_name, name }
  backend/
    audio.rs        — wpctl CLI calls: fetch_audio_state, toggle_mute, set_default_sink, list_sinks_and_default
    mpris.rs        — zbus proxies + list_players, get_playback_status, play_pause
  subscriptions/
    audio.rs        — Subscription::run_with; D-Bus-driven (org.PulseAudio1) + polling fallback
    mpris.rs        — Subscription::run_with; 1s polling; last-active player heuristic
```

## Key libcosmic API Patterns

### Application trait

```rust
impl cosmic::Application for AppModel {
    type Executor = cosmic::SingleThreadExecutor;  // required for applets
    type Flags = ();
    type Message = Message;
    const APP_ID: &'static str = "io.github.akuzko.skadi-applet";

    fn style(&self) -> Option<cosmic::iced::theme::Style> {
        Some(cosmic::applet::style())
    }
}
```

### Task type

`cosmic::app::Task<M>` = `iced::Task<cosmic::Action<M>>`. Return `Task::none()` or `cosmic::task::future(async { ... })` or `cosmic::task::message(...)`.

### Popup lifecycle (new API, NOT the old iced_winit API)

Popups are **created inside `view()`** using `on_press_with_rectangle`. Never create popups in `update()`.

```rust
use cosmic::surface::action::{app_popup, destroy_popup};
use cosmic::iced::window::Id;

// In Message enum:
Surface(cosmic::surface::Action),
PopupClosed(Id),

// In update():
Message::Surface(action) => {
    return cosmic::task::message(cosmic::Action::Cosmic(
        cosmic::app::Action::Surface(action),
    ));
}
Message::PopupClosed(id) => {
    if self.popup.as_ref() == Some(&id) { self.popup = None; }
}

// In view():
self.core.applet.icon_button("icon-name-symbolic")
    .on_press_with_rectangle(move |offset, bounds| {
        if let Some(id) = have_popup {
            Message::Surface(destroy_popup(id))
        } else {
            Message::Surface(app_popup::<AppModel>(
                move |state: &mut AppModel| {
                    let new_id = Id::unique();
                    state.popup = Some(new_id);
                    let mut s = state.core.applet.get_popup_settings(
                        state.core.main_window_id().unwrap(),
                        new_id, None, None, None,
                    );
                    s.positioner.anchor_rect = Rectangle {
                        x: (bounds.x - offset.x) as i32,
                        y: (bounds.y - offset.y) as i32,
                        width: bounds.width as i32,
                        height: bounds.height as i32,
                    };
                    s.positioner.size_limits = Limits::NONE
                        .max_width(400.0).min_width(280.0)
                        .min_height(100.0).max_height(600.0);
                    s
                },
                None::<Box<dyn for<'a> Fn(&'a AppModel) -> cosmic::Element<'a, cosmic::Action<Message>> + Send + Sync + 'static>>,
            ))
        }
    })
    .into()

// Popup content in:
fn view_window(&self, _id: Id) -> Element<'_, Self::Message> {
    self.core.applet.popup_container(content).into()
}
```

### Widget builders

Use the **builder pattern** — `widget::Column::new().push(...)` and `widget::Row::new().push(...)`.
Do NOT use `widget::column(vec![...])` or `widget::row(vec![...])` — those have a different signature.

### Subscriptions

```rust
Subscription::run_with(std::any::TypeId::of::<UniqueMarkerStruct>(), |_| {
    stream::channel(16, |mut tx: Sender<MyType>| async move {
        // tx is futures::channel::mpsc::Sender<MyType>
        // use tx.send(value).await
    })
})
```

- Do NOT use `Subscription::run_with_id` — that API does not exist in this libcosmic version.
- The `Sender<T>` type is `futures::channel::mpsc::Sender<T>` (from the `futures` crate), not `futures_util`.

### on_close_requested

```rust
fn on_close_requested(&self, id: Id) -> Option<Message> {
    Some(Message::PopupClosed(id))
}
```

## Audio Backend

- All audio operations go through **wpctl CLI** (PipeWire's WirePlumber control tool)
- `wpctl get-volume @DEFAULT_AUDIO_SINK@` — detect `[MUTED]` in output for mute status
- `wpctl set-mute @DEFAULT_AUDIO_SINK@ toggle` — toggle mute
- `wpctl set-default <id>` — switch default sink
- `wpctl status` — parse "Audio > Sinks:" section; `*` prefix marks default; extract `ID. Name [vol: x.xx]`
- Tree characters (`│├└─`) must be stripped when parsing
- `fetch_audio_state()` runs mute check and sink listing in parallel via `tokio::join!`

## MPRIS Backend

- Uses **zbus 4** `#[proxy]` macros for type-safe D-Bus proxies
- Bus name prefix: `org.mpris.MediaPlayer2.`
- `MediaPlayer2` interface: `Identity` property
- `MediaPlayer2.Player` interface: `PlaybackStatus` property, `PlayPause` method
- `list_players` enumerates all bus names matching the prefix
- Each D-Bus call opens its own connection or reuses a passed `&zbus::Connection`

## Audio Subscription Strategy

1. Connect to D-Bus session bus
2. Check if `org.PulseAudio1` is available (PipeWire PA compatibility module)
3. If yes: subscribe to `PropertiesChanged` signals → re-fetch full state via wpctl on each signal
4. If no (or on any error): fall back to 1-second polling loop
5. Both paths use `run_dbus_driven` / `run_polling` helpers with generic `S: Sink<AudioState> + Unpin` bounds

## MPRIS Subscription Strategy

- 1-second polling loop via `tokio::time::interval`
- Active player heuristic:
  1. Any player currently `"Playing"` → wins
  2. Previously tracked player still in list → keep it
  3. Fallback to first player found

## Common Pitfalls

- `cosmic::iced_winit` module does **not** exist in this libcosmic version. Use `cosmic::surface::action`.
- `cosmic::iced_runtime::Appearance` does **not** exist. Use `cosmic::iced::theme::Style`.
- `Subscription::run_with_id` does **not** exist. Use `Subscription::run_with(data, |_| stream)`.
- `widget::column()` / `widget::row()` with no args does **not** compile. Use `widget::Column::new()`.
- `futures_util::channel::mpsc::Sender` will not resolve — import from `futures::channel::mpsc::Sender`.
- Applets require `type Executor = cosmic::SingleThreadExecutor`, not the default.
- `style()` must return `Option<cosmic::iced::theme::Style>`, not `Appearance`.

## Building & Running

```sh
# Check for errors
cargo check

# Run tests (includes wpctl parser unit test)
cargo test

# Run standalone (no panel embedding)
RUST_BACKTRACE=full cargo run --release

# Install locally (no sudo)
just install rootdir=$HOME/.local prefix=''
# Installs binary to ~/.local/bin/skadi-applet
# Installs .desktop to ~/.local/share/applications/
```

## Resources

- `resources/app.desktop` — `X-CosmicApplet=true`, `X-CosmicHoverPopup=Auto`, `NoDisplay=true`
- `resources/icon.svg` — applet icon
- libcosmic checkout (for API reference): check Cargo.lock for the exact git revision, then find it under `~/.asdf/installs/rust/<version>/git/checkouts/libcosmic-*/`
- Official example: `examples/applet/src/window.rs` in the libcosmic checkout
