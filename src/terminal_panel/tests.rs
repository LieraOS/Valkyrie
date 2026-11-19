// SPDX-License-Identifier: GPL-3.0-or-later
// Valkyrie — the LieraOS file manager
// Copyright (C) 2026 Jörg Schröder, LieraOS Project
//
// Tests for the terminal panel's pure helpers.

use super::{commit_clears_line, shell_quote};

#[test]
fn quote_plain_path() {
    assert_eq!(shell_quote("/home/user/Bilder"), "'/home/user/Bilder'");
}

#[test]
fn quote_path_with_spaces() {
    assert_eq!(
        shell_quote("/home/user/My Files/x"),
        "'/home/user/My Files/x'"
    );
}

#[test]
fn quote_embedded_single_quote() {
    // ' becomes '\'' — close quote, escaped quote, reopen.
    assert_eq!(shell_quote("/tmp/it's"), "'/tmp/it'\\''s'");
}

#[test]
fn quote_neutralises_shell_metachars() {
    // Everything dangerous must stay inside the quotes.
    assert_eq!(shell_quote("$(rm -rf /);`id`&|"), "'$(rm -rf /);`id`&|'");
}

#[test]
fn commit_enter_clears() {
    // Plain Enter and text ending in Enter both end the edit line.
    assert!(commit_clears_line(b"\r"));
    assert!(commit_clears_line(b"ls -la\r"));
    assert!(commit_clears_line(b"\n"));
}

#[test]
fn commit_line_kills_clear() {
    // Ctrl+C (SIGINT) and Ctrl+U (kill line) produce a fresh prompt.
    assert!(commit_clears_line(b"\x03"));
    assert!(commit_clears_line(b"\x15"));
}

#[test]
fn commit_edit_keys_stay_dirty() {
    // Printable text, Backspace and arrows may leave buffer content —
    // the safe answer is "line not cleared".
    assert!(!commit_clears_line(b"a"));
    assert!(!commit_clears_line(b"\x7f"));
    assert!(!commit_clears_line(b"\x1b[A"));
    assert!(!commit_clears_line(b""));
}
