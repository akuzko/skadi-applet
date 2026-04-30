// SPDX-License-Identifier: MIT

mod app;
mod backend;
mod subscriptions;
mod types;
mod i18n;

fn main() -> cosmic::iced::Result {
    // Initialize tracing.
    // Default filter: DEBUG from this crate, WARN from everything else.
    // Silence cosmic::theme, which logs ERRORs when config files are absent
    // outside the full COSMIC desktop (harmless — libcosmic falls back to defaults).
    // Override at any time with the RUST_LOG env var.
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
                // cosmic::theme  — logs ERRORs for missing config files outside full COSMIC desktop (harmless)
                // winit_wayland::window::state — warns about xdg_toplevel_icon_manager_v1 (compositor doesn't
                //   support protocol; cosmetic only, irrelevant when embedded in COSMIC panel)
                "skadi_applet=debug,cosmic::theme=off,winit_wayland::window::state=off,warn"
                    .parse()
                    .unwrap()
            }),
        )
        .init();

    // Get the system's preferred languages.
    let requested_languages = i18n_embed::DesktopLanguageRequester::requested_languages();

    // Enable localizations to be applied.
    i18n::init(&requested_languages);

    // Starts the applet's event loop with `()` as the application's flags.
    cosmic::applet::run::<app::AppModel>(())
}
