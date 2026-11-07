// Tests for the terminal panel's pure helpers.

use super::shell_quote;

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
