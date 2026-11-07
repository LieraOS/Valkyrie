// SPDX-License-Identifier: GPL-3.0-or-later
// Valkyrie — the LieraOS file manager
// Copyright (C) 2026 Jörg Schröder, LieraOS Project
//
// Pure status-line formatting for the browser tab footer: total row
// count, and while a selection is active the marked item count plus
// their combined byte size. Kept in its own module so the wording is
// unit-testable without a display.

use crate::i18n::trn;

/// Status line for `count` rows with `sel` selected items totalling
/// `bytes` bytes. Directory sizes are excluded upstream (counting
/// them would need a recursive scan per selection change).
pub fn status_text(count: u32, sel: u32, bytes: u64) -> String {
    let total = trn("1 item", "%d items", count).replace("%d", &count.to_string());
    if sel == 0 {
        return total;
    }
    let mut marked =
        trn("1 item selected", "%d items selected", sel).replace("%d", &sel.to_string());
    if bytes > 0 {
        marked = format!("{marked} ({})", glib::format_size(bytes));
    }
    format!("{total} — {marked}")
}

#[cfg(test)]
mod tests {
    use super::status_text;

    #[test]
    fn no_selection_reports_total_only() {
        assert_eq!(status_text(1, 0, 0), "1 item");
        assert_eq!(status_text(5, 0, 0), "5 items");
    }

    #[test]
    fn single_selection_reports_count_and_size() {
        let text = status_text(5, 1, 512);
        assert!(text.starts_with("5 items — "), "{text}");
        assert!(text.contains("1 item selected"), "{text}");
        assert!(text.contains("bytes"), "{text}");
    }

    #[test]
    fn multi_selection_pluralizes() {
        let text = status_text(5, 3, 1024);
        assert!(text.contains("3 items selected"), "{text}");
    }

    #[test]
    fn selection_without_bytes_hides_size() {
        let text = status_text(5, 2, 0);
        assert!(text.contains("2 items selected"), "{text}");
        assert!(!text.contains('('), "{text}");
    }
}
