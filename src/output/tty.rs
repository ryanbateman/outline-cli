//! TTY detection and terminal properties.

use std::io::IsTerminal;

use super::color::ColorMode;

/// Check if stdout is a TTY.
pub fn is_tty() -> bool {
    std::io::stdout().is_terminal()
}

/// Check if color output should be enabled.
pub fn use_color(mode: ColorMode) -> bool {
    match mode {
        ColorMode::Always => true,
        ColorMode::Never => false,
        ColorMode::Auto => is_tty() && std::env::var("NO_COLOR").is_err(),
    }
}

/// Get the terminal width in columns.
/// Falls back to 80 if detection fails or stdout is not a TTY.
pub fn terminal_width() -> usize {
    terminal_size::terminal_size()
        .map(|(w, _)| w.0 as usize)
        .unwrap_or(80)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_width_returns_reasonable_value() {
        let width = terminal_width();
        assert!(
            width >= 20,
            "Terminal width {width} seems unreasonably small"
        );
        assert!(
            width <= 1000,
            "Terminal width {width} seems unreasonably large"
        );
    }

    #[test]
    fn color_mode_always_enables_color() {
        assert!(use_color(ColorMode::Always));
    }

    #[test]
    fn color_mode_never_disables_color() {
        assert!(!use_color(ColorMode::Never));
    }
}
