// SPDX-License-Identifier: MPL-2.0

mod models;
mod ui;

fn main() {
    let theme = ui::theme::kotodama_theme();
    println!("Kotodama v{} ({})", env!("CARGO_PKG_VERSION"), theme);
}
