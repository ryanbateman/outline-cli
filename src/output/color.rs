//! ANSI color and style helpers.
//!
//! All functions respect `ColorMode` — when color is disabled, strings pass through unchanged.
//! Uses raw ANSI escape codes to avoid extra dependencies.

// All color functions are part of the public style API; some are used only in tests or
// in the library crate.
#![allow(dead_code)]

/// Color output mode.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ColorMode {
    /// Detect from TTY and NO_COLOR env var.
    Auto,
    /// Always emit color codes.
    Always,
    /// Never emit color codes.
    Never,
}

impl ColorMode {
    pub fn from_str_arg(s: &str) -> Self {
        match s {
            "always" => ColorMode::Always,
            "never" => ColorMode::Never,
            _ => ColorMode::Auto,
        }
    }
}

// ANSI escape codes
const RESET: &str = "\x1b[0m";
const BOLD: &str = "\x1b[1m";
const DIM: &str = "\x1b[2m";
const UNDERLINE: &str = "\x1b[4m";
const RED: &str = "\x1b[31m";
const GREEN: &str = "\x1b[32m";
const YELLOW: &str = "\x1b[33m";
const BLUE: &str = "\x1b[34m";
const CYAN: &str = "\x1b[36m";

/// Wrap a string with ANSI codes if color is enabled.
fn styled(s: &str, codes: &str, color_enabled: bool) -> String {
    if color_enabled {
        format!("{codes}{s}{RESET}")
    } else {
        s.to_string()
    }
}

pub fn bold(s: &str, color: bool) -> String {
    styled(s, BOLD, color)
}

pub fn dim(s: &str, color: bool) -> String {
    styled(s, DIM, color)
}

pub fn red(s: &str, color: bool) -> String {
    styled(s, RED, color)
}

pub fn red_bold(s: &str, color: bool) -> String {
    styled(s, &format!("{RED}{BOLD}"), color)
}

pub fn green(s: &str, color: bool) -> String {
    styled(s, GREEN, color)
}

pub fn yellow(s: &str, color: bool) -> String {
    styled(s, YELLOW, color)
}

pub fn blue(s: &str, color: bool) -> String {
    styled(s, BLUE, color)
}

pub fn cyan(s: &str, color: bool) -> String {
    styled(s, CYAN, color)
}

pub fn cyan_underline(s: &str, color: bool) -> String {
    styled(s, &format!("{CYAN}{UNDERLINE}"), color)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bold_with_color_wraps_in_ansi() {
        let result = bold("hello", true);
        assert!(result.starts_with("\x1b[1m"));
        assert!(result.ends_with("\x1b[0m"));
        assert!(result.contains("hello"));
    }

    #[test]
    fn bold_without_color_passes_through() {
        assert_eq!(bold("hello", false), "hello");
    }

    #[test]
    fn red_bold_combines_codes() {
        let result = red_bold("error", true);
        assert!(result.contains("\x1b[31m"));
        assert!(result.contains("\x1b[1m"));
        assert!(result.contains("error"));
    }

    #[test]
    fn all_styles_passthrough_when_disabled() {
        assert_eq!(dim("x", false), "x");
        assert_eq!(red("x", false), "x");
        assert_eq!(green("x", false), "x");
        assert_eq!(yellow("x", false), "x");
        assert_eq!(blue("x", false), "x");
        assert_eq!(cyan("x", false), "x");
        assert_eq!(cyan_underline("x", false), "x");
    }

    #[test]
    fn color_mode_from_str() {
        assert_eq!(ColorMode::from_str_arg("always"), ColorMode::Always);
        assert_eq!(ColorMode::from_str_arg("never"), ColorMode::Never);
        assert_eq!(ColorMode::from_str_arg("auto"), ColorMode::Auto);
        assert_eq!(ColorMode::from_str_arg("other"), ColorMode::Auto);
    }
}
