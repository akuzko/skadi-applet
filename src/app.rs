// SPDX-License-Identifier: MIT

use cosmic::app::{Core, Task};
use cosmic::iced::window::Id;
use cosmic::iced::{Length, Limits, Rectangle};
use cosmic::surface::action::{app_popup, destroy_popup};
use cosmic::widget;
use cosmic::Element;
use tracing::warn;

use crate::backend::audio::{fetch_audio_state, set_default_sink, toggle_mute};
use crate::backend::mpris::{next_track, play_pause, previous_track};
use crate::subscriptions::audio::audio_subscription;
use crate::subscriptions::mpris::{MprisState, mpris_subscription};
use crate::types::{AudioDevice, MprisPlayer, TrackInfo};

// ── Application model ─────────────────────────────────────────────────────────

#[derive(Default)]
pub struct AppModel {
    core: Core,
    popup: Option<Id>,

    // Audio state
    is_muted: bool,
    devices: Vec<AudioDevice>,
    default_device_id: Option<u32>,

    // MPRIS state
    active_player: Option<MprisPlayer>,
    is_playing: bool,
    track_info: Option<TrackInfo>,
    can_go_previous: bool,
    can_go_next: bool,
}

// ── Messages ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum Message {
    // Surface (popup lifecycle delegated to COSMIC runtime)
    Surface(cosmic::surface::Action),
    PopupClosed(Id),

    // User actions
    ToggleMute,
    SwitchDevice(u32),
    PlayPause,
    PreviousTrack,
    NextTrack,

    // State updates from subscriptions
    AudioStateUpdate(crate::backend::audio::AudioState),
    MprisUpdate(MprisState),

    // Command results (errors are logged, not shown in UI for MVP)
    CommandResult(Result<(), String>),
}

// ── Application impl ──────────────────────────────────────────────────────────

impl cosmic::Application for AppModel {
    type Executor = cosmic::SingleThreadExecutor;
    type Flags = ();
    type Message = Message;
    const APP_ID: &'static str = "io.github.akuzko.skadi-applet";

    fn core(&self) -> &Core {
        &self.core
    }

    fn core_mut(&mut self) -> &mut Core {
        &mut self.core
    }

    fn init(core: Core, _flags: Self::Flags) -> (Self, Task<Message>) {
        let app = Self {
            core,
            ..Default::default()
        };

        // Fetch initial audio state immediately
        let init_task = cosmic::task::future(async {
            let state = fetch_audio_state().await.unwrap_or_else(|e| {
                warn!("initial audio state fetch failed: {e}");
                crate::backend::audio::AudioState {
                    is_muted: false,
                    devices: vec![],
                    default_device_id: None,
                }
            });
            Message::AudioStateUpdate(state)
        });

        (app, init_task)
    }

    fn on_close_requested(&self, id: Id) -> Option<Message> {
        Some(Message::PopupClosed(id))
    }

    fn subscription(&self) -> cosmic::iced::Subscription<Self::Message> {
        cosmic::iced::Subscription::batch([
            audio_subscription().map(Message::AudioStateUpdate),
            mpris_subscription().map(Message::MprisUpdate),
        ])
    }

    fn update(&mut self, message: Self::Message) -> Task<Message> {
        match message {
            // ── Surface action (popup create/destroy) ──────────────────────
            Message::Surface(action) => {
                return cosmic::task::message(cosmic::Action::Cosmic(
                    cosmic::app::Action::Surface(action),
                ));
            }

            Message::PopupClosed(id) => {
                if self.popup.as_ref() == Some(&id) {
                    self.popup = None;
                }
            }

            // ── User actions ───────────────────────────────────────────────
            Message::ToggleMute => {
                return cosmic::task::future(async {
                    let result = toggle_mute().await.map_err(|e| e.to_string());
                    Message::CommandResult(result)
                });
            }

            Message::SwitchDevice(id) => {
                return cosmic::task::future(async move {
                    let result = set_default_sink(id).await.map_err(|e| e.to_string());
                    Message::CommandResult(result)
                });
            }

            Message::PlayPause => {
                if let Some(player) = self.active_player.clone() {
                    return cosmic::task::future(async move {
                        let conn = match zbus::Connection::session().await {
                            Ok(c) => c,
                            Err(e) => return Message::CommandResult(Err(e.to_string())),
                        };
                        let result = play_pause(&conn, &player.bus_name)
                            .await
                            .map_err(|e| e.to_string());
                        Message::CommandResult(result)
                    });
                }
            }

            Message::PreviousTrack => {
                if let Some(player) = self.active_player.clone() {
                    return cosmic::task::future(async move {
                        let conn = match zbus::Connection::session().await {
                            Ok(c) => c,
                            Err(e) => return Message::CommandResult(Err(e.to_string())),
                        };
                        let result = previous_track(&conn, &player.bus_name)
                            .await
                            .map_err(|e| e.to_string());
                        Message::CommandResult(result)
                    });
                }
            }

            Message::NextTrack => {
                if let Some(player) = self.active_player.clone() {
                    return cosmic::task::future(async move {
                        let conn = match zbus::Connection::session().await {
                            Ok(c) => c,
                            Err(e) => return Message::CommandResult(Err(e.to_string())),
                        };
                        let result = next_track(&conn, &player.bus_name)
                            .await
                            .map_err(|e| e.to_string());
                        Message::CommandResult(result)
                    });
                }
            }

            // ── State updates ──────────────────────────────────────────────
            Message::AudioStateUpdate(state) => {
                self.is_muted = state.is_muted;
                self.devices = state.devices;
                self.default_device_id = state.default_device_id;
            }

            Message::MprisUpdate(state) => {
                self.active_player = state.active_player;
                self.is_playing = state.is_playing;
                self.track_info = state.track_info;
                self.can_go_previous = state.can_go_previous;
                self.can_go_next = state.can_go_next;
            }

            Message::CommandResult(Err(e)) => {
                warn!("command failed: {e}");
            }

            Message::CommandResult(Ok(())) => {}
        }

        Task::none()
    }

    // ── Panel icon ─────────────────────────────────────────────────────────

    fn view(&self) -> Element<'_, Self::Message> {
        let icon_name = if self.is_muted {
            "audio-volume-muted-symbolic"
        } else {
            "audio-volume-high-symbolic"
        };

        let have_popup = self.popup;

        self.core
            .applet
            .icon_button(icon_name)
            .on_press_with_rectangle(move |offset, bounds| {
                if let Some(id) = have_popup {
                    Message::Surface(destroy_popup(id))
                } else {
                    Message::Surface(app_popup::<AppModel>(
                        move |state: &mut AppModel| {
                            let new_id = Id::unique();
                            state.popup = Some(new_id);
                            let mut popup_settings = state.core.applet.get_popup_settings(
                                state.core.main_window_id().unwrap(),
                                new_id,
                                None,
                                None,
                                None,
                            );
                            popup_settings.positioner.anchor_rect = Rectangle {
                                x: (bounds.x - offset.x) as i32,
                                y: (bounds.y - offset.y) as i32,
                                width: bounds.width as i32,
                                height: bounds.height as i32,
                            };
                            popup_settings.positioner.size_limits = Limits::NONE
                                .max_width(400.0)
                                .min_width(280.0)
                                .min_height(100.0)
                                .max_height(600.0);
                            popup_settings
                        },
                        None::<Box<dyn for<'a> Fn(&'a AppModel) -> cosmic::Element<'a, cosmic::Action<Message>> + Send + Sync + 'static>>,
                    ))
                }
            })
            .into()
    }

    // ── Popup content ──────────────────────────────────────────────────────

    fn view_window(&self, _id: Id) -> Element<'_, Self::Message> {
        let mut content = widget::Column::new()
            .push(self.audio_section())
            .padding(16)
            .spacing(8);

        if let Some(media) = self.media_section() {
            content = content
                .push(widget::divider::horizontal::default())
                .push(media);
        }

        self.core.applet.popup_container(content).into()
    }

    fn style(&self) -> Option<cosmic::iced::theme::Style> {
        Some(cosmic::applet::style())
    }
}

// ── UI helpers ────────────────────────────────────────────────────────────────

impl AppModel {
    fn audio_section(&self) -> Element<'_, Message> {
        let mute_icon = if self.is_muted {
            "audio-volume-muted-symbolic"
        } else {
            "audio-volume-medium-symbolic"
        };
        let mute_label = if self.is_muted { "Unmute" } else { "Mute" };

        let mute_button = widget::button::custom(
            widget::container(
                widget::Row::new()
                    .push(widget::icon::from_name(mute_icon).size(24))
                    .push(widget::text(mute_label))
                    .spacing(8)
                    .align_y(cosmic::iced::Alignment::Center),
            )
            .align_x(cosmic::iced::Alignment::Center)
            .width(Length::Fill),
        )
        .padding([8, 16])
        .on_press(Message::ToggleMute);

        let header_row = widget::Row::new()
            .push(widget::text::heading("Audio").width(Length::Fill))
            .push(mute_button)
            .align_y(cosmic::iced::Alignment::Center);

        let mut col = widget::Column::new()
            .push(header_row)
            .spacing(8);

        if self.devices.is_empty() {
            col = col.push(widget::text("No output devices found").size(12));
        } else {
            let mut list = widget::list_column();
            for device in &self.devices {
                let is_default = self.default_device_id == Some(device.id);
                let device_id = device.id;
                list = list.add(
                    widget::list::button(widget::text(device.name.clone()).size(13))
                        .on_press(Message::SwitchDevice(device_id))
                        .selected(is_default),
                );
            }
            col = col.push(list);
        }

        col.into()
    }

    fn media_section(&self) -> Option<Element<'_, Message>> {
        let player = self.active_player.as_ref()?;

        let header = widget::text::heading(format!("Media  \u{2B29}  {}", player.name));

        // ── Track info ─────────────────────────────────────────────────────
        let mut col = widget::Column::new()
            .push(header)
            .spacing(4);

        if let Some(track_info) = &self.track_info {
            if let Some(title) = &track_info.title {
                col = col.push(widget::text(title.clone()).size(13));
            }
            if let Some(artist) = &track_info.artist {
                col = col.push(
                    widget::text(artist.clone())
                        .size(11)
                        .class(cosmic::theme::Text::Default),
                );
            }
        }

        // ── Controls row: Previous | Play/Pause | Next ─────────────────────
        let play_icon = if self.is_playing {
            "media-playback-pause-symbolic"
        } else {
            "media-playback-start-symbolic"
        };

        let mut controls = widget::Row::new().spacing(4);

        let prev_btn = widget::button::icon(
            widget::icon::from_name("media-skip-backward-symbolic").size(24),
        );
        controls = controls.push(if self.can_go_previous {
            prev_btn.on_press(Message::PreviousTrack)
        } else {
            prev_btn
        });

        controls = controls.push(
            widget::button::icon(widget::icon::from_name(play_icon).size(24))
                .on_press(Message::PlayPause),
        );

        let next_btn = widget::button::icon(
            widget::icon::from_name("media-skip-forward-symbolic").size(24),
        );
        controls = controls.push(if self.can_go_next {
            next_btn.on_press(Message::NextTrack)
        } else {
            next_btn
        });

        col = col.push(
            widget::container(controls)
                .align_x(cosmic::iced::Alignment::Center)
                .width(Length::Fill),
        );

        Some(col.into())
    }
}
