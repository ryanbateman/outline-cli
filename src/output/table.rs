//! Borderless table renderer for CLI list output.
//!
//! Produces clean output like `kubectl get` or `docker ps`:
//! - Header row in bold (when color enabled)
//! - Auto-sized columns based on content width
//! - Truncation with "..." for values exceeding column width
//! - Two-space column separator
//! - Respects terminal width

use super::color;

const COLUMN_GAP: usize = 2;
const MIN_COLUMN_WIDTH: usize = 4;
const ELLIPSIS: &str = "...";

/// A borderless table with auto-sized columns.
pub struct Table {
    headers: Vec<String>,
    rows: Vec<Vec<String>>,
}

impl Table {
    pub fn new(headers: Vec<String>) -> Self {
        Self {
            headers,
            rows: Vec::new(),
        }
    }

    pub fn add_row(&mut self, values: Vec<String>) {
        self.rows.push(values);
    }

    /// Render the table as a string.
    ///
    /// Columns are auto-sized to fit content within `max_width`.
    /// The header row is bold when `color_enabled` is true.
    pub fn render(&self, max_width: usize, color_enabled: bool) -> String {
        if self.headers.is_empty() {
            return String::new();
        }

        let col_count = self.headers.len();

        // Calculate the natural width of each column (max of header + all values)
        let mut natural_widths: Vec<usize> = self.headers.iter().map(|h| h.len()).collect();

        for row in &self.rows {
            for (i, val) in row.iter().enumerate() {
                if i < col_count {
                    natural_widths[i] = natural_widths[i].max(val.len());
                }
            }
        }

        // Fit columns within max_width
        let widths = fit_columns(&natural_widths, max_width);

        let mut lines = Vec::new();

        // Header row
        let header_line = render_row(&self.headers, &widths);
        lines.push(color::bold(&header_line, color_enabled));

        // Data rows
        for row in &self.rows {
            lines.push(render_row(row, &widths));
        }

        lines.join("\n")
    }
}

/// Render a single row with the given column widths.
fn render_row(values: &[String], widths: &[usize]) -> String {
    let mut parts = Vec::new();

    for (i, width) in widths.iter().enumerate() {
        let val = values.get(i).map(|s| s.as_str()).unwrap_or("");
        let truncated = truncate(val, *width);
        // Left-align, pad to column width
        parts.push(format!("{:<width$}", truncated, width = width));
    }

    // Join with gap, trim trailing whitespace
    parts.join(&" ".repeat(COLUMN_GAP)).trim_end().to_string()
}

/// Truncate a string to fit within `max_len`, appending "..." if truncated.
fn truncate(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else if max_len <= ELLIPSIS.len() {
        ELLIPSIS[..max_len].to_string()
    } else {
        let end = max_len - ELLIPSIS.len();
        format!("{}{ELLIPSIS}", &s[..end])
    }
}

/// Fit columns within the available width.
///
/// Strategy:
/// 1. Start with natural widths.
/// 2. If total exceeds max_width, proportionally shrink columns.
/// 3. Enforce minimum column width.
fn fit_columns(natural: &[usize], max_width: usize) -> Vec<usize> {
    let col_count = natural.len();
    if col_count == 0 {
        return vec![];
    }

    let gap_space = COLUMN_GAP * (col_count.saturating_sub(1));
    let available = max_width.saturating_sub(gap_space);

    let total_natural: usize = natural.iter().sum();

    if total_natural <= available {
        // Everything fits — use natural widths
        return natural.to_vec();
    }

    // Proportionally shrink
    let mut widths: Vec<usize> = natural
        .iter()
        .map(|&w| {
            let proportion = w as f64 / total_natural as f64;
            let scaled = (proportion * available as f64).floor() as usize;
            scaled.max(MIN_COLUMN_WIDTH)
        })
        .collect();

    // Adjust rounding: if we're over budget, shrink the widest column
    let total: usize = widths.iter().sum();
    if total > available
        && let Some(max_idx) = widths
            .iter()
            .enumerate()
            .max_by_key(|(_, w)| *w)
            .map(|(i, _)| i)
    {
        let excess = total - available;
        widths[max_idx] = widths[max_idx].saturating_sub(excess).max(MIN_COLUMN_WIDTH);
    }

    widths
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_table_renders_empty() {
        let table = Table::new(vec![]);
        assert_eq!(table.render(80, false), "");
    }

    #[test]
    fn single_row_table() {
        let mut table = Table::new(vec!["ID".into(), "NAME".into()]);
        table.add_row(vec!["abc".into(), "Test".into()]);
        let output = table.render(80, false);
        let lines: Vec<&str> = output.lines().collect();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].contains("ID"));
        assert!(lines[0].contains("NAME"));
        assert!(lines[1].contains("abc"));
        assert!(lines[1].contains("Test"));
    }

    #[test]
    fn columns_auto_size() {
        let mut table = Table::new(vec!["A".into(), "LONG_HEADER".into()]);
        table.add_row(vec!["x".into(), "short".into()]);
        let output = table.render(80, false);
        let lines: Vec<&str> = output.lines().collect();
        // Header should be wider than "A" because LONG_HEADER dominates
        assert!(lines[0].len() > 5);
    }

    #[test]
    fn truncation_works() {
        assert_eq!(truncate("hello world", 5), "he...");
        assert_eq!(truncate("hi", 5), "hi");
        assert_eq!(truncate("hello", 5), "hello");
    }

    #[test]
    fn truncation_very_small() {
        assert_eq!(truncate("hello", 2), "..");
        assert_eq!(truncate("hello", 3), "...");
    }

    #[test]
    fn narrow_terminal_still_renders() {
        let mut table = Table::new(vec!["ID".into(), "TITLE".into(), "URL".into()]);
        table.add_row(vec![
            "abc-def-123".into(),
            "A Very Long Title".into(),
            "https://example.com/very/long/path".into(),
        ]);
        let output = table.render(40, false);
        // Should still produce output without panicking
        assert!(!output.is_empty());
        // Lines should not exceed max width by too much (some overflow is ok due to min widths)
    }

    #[test]
    fn fit_columns_within_budget() {
        let natural = vec![10, 20, 30];
        let widths = fit_columns(&natural, 80);
        assert_eq!(widths, vec![10, 20, 30]); // all fit
    }

    #[test]
    fn fit_columns_shrinks_proportionally() {
        let natural = vec![40, 40, 40]; // 120 + 4 gap = 124
        let widths = fit_columns(&natural, 50);
        let total: usize = widths.iter().sum();
        // Should be <= available (50 - 4 gap = 46)
        assert!(total <= 46, "total {total} exceeds available 46");
    }

    #[test]
    fn header_is_bold_when_color_enabled() {
        let mut table = Table::new(vec!["NAME".into()]);
        table.add_row(vec!["test".into()]);
        let with_color = table.render(80, true);
        let without_color = table.render(80, false);
        // With color should contain ANSI bold code
        assert!(with_color.contains("\x1b[1m"));
        assert!(!without_color.contains("\x1b["));
    }

    #[test]
    fn missing_values_render_as_empty() {
        let mut table = Table::new(vec!["A".into(), "B".into(), "C".into()]);
        table.add_row(vec!["x".into()]); // only 1 of 3 columns
        let output = table.render(80, false);
        assert!(output.contains("x"));
    }
}
