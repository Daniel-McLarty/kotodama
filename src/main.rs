// SPDX-License-Identifier: MPL-2.0

mod anki;
mod audio;
mod models;
mod storage;
mod ui;

use std::process::ExitCode;

use ui::KotodamaApp;

fn main() -> ExitCode {
    let result = iced::application(KotodamaApp::boot, KotodamaApp::update, KotodamaApp::view)
        .title(KotodamaApp::title)
        .theme(KotodamaApp::theme)
        .subscription(KotodamaApp::subscription)
        .window_size((1080.0, 720.0))
        .run();

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("Kotodama failed to launch desktop window: {err}");
            ExitCode::FAILURE
        }
    }
}
