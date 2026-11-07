// SPDX-License-Identifier: GPL-3.0-or-later
// Valkyrie — the LieraOS file manager
// Copyright (C) 2026 Jörg Schröder, LieraOS Project
//
// Secret storage through the system keyring (GNOME Keyring /
// KWallet — anything serving org.freedesktop.secrets). Valkyrie never
// keeps passwords in GSettings or files; they go into the encrypted
// login collection via `secret-tool`, with the password itself piped
// through stdin so it never appears in a process command line.

use std::io::Write;
use std::process::{Command, Stdio};

/// Attribute name marking a Valkyrie secret; the value identifies
/// the account type (currently only "nextcloud").
const SERVICE_ATTR: &str = "service";
const SERVICE_NEXTCLOUD: &str = "org.lieraos.valkyrie.nextcloud";

/// Store `password` for (server, user) in the encrypted keyring.
/// Returns Err with stderr text when the daemon is unavailable.
pub fn store(server: &str, user: &str, password: &str) -> Result<(), String> {
    let mut child = Command::new("secret-tool")
        .args([
            "store",
            &format!("--label=Valkyrie Cloud ({user}@{server})"),
            SERVICE_ATTR,
            SERVICE_NEXTCLOUD,
            "server",
            server,
            "user",
            user,
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| e.to_string())?;
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(password.as_bytes());
        // stdin is dropped here, closing the pipe.
    }
    let out = child.wait_with_output().map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

/// Read the stored password for (server, user); None when absent.
pub fn lookup(server: &str, user: &str) -> Option<String> {
    let out = Command::new("secret-tool")
        .args([
            "lookup",
            SERVICE_ATTR,
            SERVICE_NEXTCLOUD,
            "server",
            server,
            "user",
            user,
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !out.status.success() || out.stdout.is_empty() {
        return None;
    }
    String::from_utf8(out.stdout).ok()
}

/// Whether a password exists for (server, user) — used to show the
/// "stored" state in preferences without revealing the secret.
pub fn has(server: &str, user: &str) -> bool {
    lookup(server, user).is_some()
}

/// Remove the stored password for (server, user).
pub fn clear(server: &str, user: &str) {
    let _ = Command::new("secret-tool")
        .args([
            "clear",
            SERVICE_ATTR,
            SERVICE_NEXTCLOUD,
            "server",
            server,
            "user",
            user,
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Roundtrip through the real session keyring; skipped silently
    /// when no org.freedesktop.secrets service is reachable (CI).
    #[test]
    fn store_lookup_clear_roundtrip() {
        let server = "test.example.invalid";
        let user = "valkyrie-ci";
        if store(server, user, "s3cret").is_err() {
            return; // no keyring in this environment
        }
        assert_eq!(lookup(server, user).as_deref(), Some("s3cret"));
        clear(server, user);
        assert!(lookup(server, user).is_none());
    }
}
