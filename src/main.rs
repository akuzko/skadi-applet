// SPDX-License-Identifier: MIT

mod app;
mod backend;
mod subscriptions;
mod types;
mod i18n;

fn main() -> cosmic::iced::Result {
    // Initialize tracing
    tracing_subscriber::fmt::init();

    // Get the system's preferred languages.
    let requested_languages = i18n_embed::DesktopLanguageRequester::requested_languages();

    // Enable localizations to be applied.
    i18n::init(&requested_languages);

    // Starts the applet's event loop with `()` as the application's flags.
    cosmic::applet::run::<app::AppModel>(())
}
