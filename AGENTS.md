# Valkyrie — Agent Notes

## Verifikation

- `cargo test`, `cargo clippy -- -D warnings`, `cargo fmt --check` —
  alle müssen sauber durchlaufen, bevor gebaut/installiert wird.
- Release-Install: `cargo build --release && cp target/release/valkyrie ~/.local/bin/`
- GSettings-Schema nach Änderungen an
  `data/org.lieraos.valkyrie.gschema.xml` neu installieren:
  `cp data/org.lieraos.valkyrie.gschema.xml ~/.local/share/glib-2.0/schemas/
  && glib-compile-schemas ~/.local/share/glib-2.0/schemas/`
- Gettext: `msgfmt po/de.po -o ~/.local/share/locale/de/LC_MESSAGES/valkyrie.mo`
  (analog `en`); Laufzeit-Suchpfad ist `~/.local/share/locale` bzw.
  `LOCALEDIR`-Override.

## UI-Tests unter Xvfb

- IMMER `NO_AT_BRIDGE=1` (oder `GTK_A11Y=none`) in Testskripten setzen:
  `dbus-run-session`-Sessions spawnen eigene at-spi-Daemons auf dem
  hartkodierten Socket `/run/user/1001/at-spi/bus_0` und unlinken ihn
  beim Beenden — danach wirft jede GTK-App der echten Session
  `Gtk-CRITICAL: Unable to connect to the accessibility bus`.
  Reparatur: stale `at-spi-*`-Prozesse killen, dann
  `systemctl --user restart at-spi-dbus-bus.service`.
- NIE fremde Desktop-Apps (GIMP etc.) innerhalb von `dbus-run-session`
  starten — selbst mit NO_AT_BRIDGE können deren eigene a11y-Brücken
  (GTK3/Qt) den geteilten Socket belegen und beim Session-Ende
  mit wegnehmen. App-Launch-Tests einmalig und bewusst durchführen,
  danach externe Apps nur außerhalb privater Sessions testen.
- Popover/Menüs rendern unter barem Xvfb unsichtbar (kein WM/
  Compositor), sind aber gemappt und Klicks dispatchen — Items per
  gemessener Geometrie (xwininfo) klicken. Accelerators via xdotool
  sind flaky; Menü-Klicks sind zuverlässiger.
