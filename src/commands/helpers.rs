use indicatif::{ProgressBar, ProgressStyle};

use dbdiff::cli::{ColorMode, SslMode};
use dbdiff::loader;

pub fn apply_color_mode(mode: ColorMode) {
    match mode {
        ColorMode::Auto => {
            if !std::io::IsTerminal::is_terminal(&std::io::stdout()) {
                colored::control::set_override(false);
            }
        }
        ColorMode::Always => {
            colored::control::set_override(true);
        }
        ColorMode::Never => {
            colored::control::set_override(false);
        }
    }
}

pub fn resolve_ssl_mode(mode: SslMode) -> loader::SslMode {
    match mode {
        SslMode::Disable => loader::SslMode::Disable,
        SslMode::Prefer => loader::SslMode::Prefer,
        SslMode::Require => loader::SslMode::Require,
    }
}

pub fn create_spinner(msg: &str) -> ProgressBar {
    let pb = ProgressBar::new_spinner();
    pb.set_style(
        ProgressStyle::with_template("{spinner:.cyan} {msg}")
            .unwrap()
            .tick_strings(&["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"]),
    );
    pb.set_message(msg.to_string());
    pb.enable_steady_tick(std::time::Duration::from_millis(80));
    pb
}
