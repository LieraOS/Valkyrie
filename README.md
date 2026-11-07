# Valkyrie

The LieraOS file manager — a GTK4 file browser written in Rust.

## Features

- Classic layout: places/devices/network sidebar on the left,
  file view with breadcrumb pathbar on the right
- Classic menu bar: File (tabs/windows, creation, properties, quit),
  Edit (clipboard on multi-selection, select all, trash), View
  (icons/list, arrange, hidden files, reload), Help (About dialog)
- Tabs (Ctrl+T / Ctrl+W) with a close button in every header and
  middle-click close; detachable between windows — dragging a tab
  outside every window opens a new window for it; the tab bar is
  hidden while only one tab exists
- Directories open in place (double click), in a new tab (middle click
  or context menu) or in a separate window (context menu)
- Rename with F2, the row context menu or the Edit menu
- Recursive search (Ctrl+F): case-insensitive subtree match on a
  worker thread, results stream into the normal view pipeline, every
  keystroke cancels the previous run
- Copy/move/delete run asynchronously on a worker thread with a
  progress window and cancel; directory copies recurse, deletes are
  post-order so non-empty folders work, same-directory drops and
  drops into a source's own subtree are filtered out. Local copies
  try a FICLONE reflink first (instant on CoW filesystems) and fall
  back to a streamed pipeline; an optional "Verify integrity" toggle
  hashes each file with BLAKE3 inline while copying and re-reads the
  destination for comparison — the source is never re-read. A
  "File operations" settings page can switch copying to a worker
  pool (user-chosen thread count, capped at CPU cores) that pulls
  file payloads from a shared queue while the tree walk stays
  sequential
- Name collisions ask the user instead of silently duplicating: a
  conflict dialog shows both files side by side (icon, size, modified
  and created dates) and offers Replace/Merge, Rename with a free name
  entry, Skip, Cancel and "apply to all" for the rest of the batch
- Multiple selection: rubber-band dragging in icon and list views,
  Ctrl/Shift click; copy, cut, trash and delete operate on the whole
  selection
- System clipboard interop: copy/cut publishes
  x-special/gnome-copied-files + text/uri-list, paste reads the same
  formats — files move between Valkyrie, the desktop and other file
  managers in both directions
- The status bar shows the item count and, while a selection is
  active, the marked count plus their combined size; selections carry
  over when switching between icon and list view
- Icon-view selection hugs the item: an app stylesheet (data/valkyrie.css,
  embedded) clears the stretched cell highlight and paints a rounded
  accent box on a compact item card; clicks beside an icon hit
  background (clears the selection), long names wrap to two lines and
  ellipsize — the real filename stays untouched and is shown as
  tooltip. The stock theme's invisible cell padding and card
  margin/border-spacing are reset and the card width stays at
  icon-plus-padding, so the column count grows and cards pack tight
  at every zoom level instead of drifting far apart
- Drag & drop: rows carry a gdk::FileList payload to folder rows, the
  view background, tab labels and other applications; a plain drop
  moves, Ctrl copies
- Permanent delete via context menu or Shift+Delete, with an
  optional secure-delete mode (Preferences → Delete): files are
  overwritten with random data N times (1–7 passes) before
  unlinking. Enabling it on unreliable media (SSD, btrfs/zfs/f2fs —
  probed via mountinfo + sysfs rotational flag) raises a one-time
  warning that overwrite is best-effort there, not a guarantee
- Mounting is the system's job, not the file manager's: volumes and
  removable media are mounted by the desktop session; Valkyrie only
  lists devices in the sidebar
- Bookmarks: any directory can be pinned to the sidebar's Bookmarks
  section via the context menu ("Add/Remove Bookmark"); the list
  persists in GSettings and refreshes live
- Archive support: "Extract Here" on archive files (zip, tar.gz, 7z,
  rar, xz, bz2 — detected by content type) and "Compress…" on any
  selection (zip / tar.gz / 7z format chooser) via 7z and tar; both
  entries are toggleable in the context-menu settings. The compress
  dialog pre-fills the source name; extraction renames a single
  top-level directory to match the archive name (so "Y.zip" extracts
  to "Y/", not the original source folder name)
- Undo (Ctrl+Z) for copy, move and trash operations — a per-window
  journal records each reversible operation and restores the original
  state; permanent delete is not undoable
- Thumbnails for PDF (via pdftoppm/poppler) and video (via
  ffmpegthumbnailer when installed) in addition to the existing image
  thumbnail cache
- Context menu is configurable (Preferences → Context menu): every
  entry except "Open" and "Properties" can be toggled per entry —
  "Open in Terminal" (external) and "Open Terminal Panel" (embedded
  VTE) are independent choices, hidden ids persist in GSettings
- Trash actions while browsing trash:///: restore (via
  trash::orig-path), permanently delete one item, empty the trash —
  destructive steps ask for confirmation
- Icon grid by default, details list (Name/Size/Type/Modified)
  switchable; the choice is persisted in GSettings
- Zoom slider in the status bar (16–300) — the current level lives
  in the slider's tooltip instead of a permanent readout, keeping the
  bar slim; the icon size is remembered per directory in GVFS metadata,
  fallback in GSettings (default: 80px)
- The icon grid has real gaps between items — clicks between cards
  hit background and clear the selection; the scroll range is
  corrected so the view never scrolls into empty space below the
  last row
- Directory enumeration skips the per-file icon attribute and
  derives icons lazily per content type (~45ms faster per 5000
  entries)
- Thumbnails: the icon grid shows real image previews — cached entries
  from other applications are reused (freedesktop thumbnail::path),
  local images get their own thumbnails written to the shared
  $XDG_CACHE_HOME/thumbnails cache
- Mouse side buttons navigate back (button 8) and forward (button 9)
- Hidden files are filtered by default; "Show Hidden Files" (Ctrl+H or
  context menu) is persisted until disabled again
- Sorting via clickable column headers or "Sort By" in the menu
  (name/size/type/modified, ascending/descending; directories
  stay first), persisted. Names sort naturally: file_2 comes
  before file_10
- Preferences dialog (Edit → Preferences, Ctrl+,): a category
  sidebar on the left (View, Sorting, Terminal, Cloud) switches
  the settings page shown on the right — each page scrolls on its
  own, so new sections never turn the dialog into one endless form.
  View (default view, hidden files, default icon size), Sorting
  (default field + direction) and Terminal (emulator picker listing
  every detected emulator, or automatic detection) are applied live
  to all open tabs
- Own identity: the Valkyrie app icon (icon/icon.png + SVG) is
  installed into the hicolor theme; the bundled "Valkyrie" icon theme
  (Theme/Valkyrie — folders, MIME types, places, devices, actions)
  becomes the app icon theme when installed, inheriting Adwaita/hicolor
  fallbacks
- Devices section fed by `GVolumeMonitor` (internal disks, USB,
  optical): clicking an unmounted volume mounts and opens it,
  mounted rows offer an eject button and a right-click menu with
  Open/Unmount/Eject
- Context menu: Open, Open With… (submenu of every app registered for
  the file's content type, icons included; "Other Application…" opens
  a filterable chooser of all installed apps that can optionally set
  the pick as the MIME type's default handler), Open as Administrator
  (`admin://` GVFS — mounted via gvfsd-admin, which triggers the
  polkit password prompt; no root process), Open in Terminal (auto-detected emulator — opens a separate
  terminal window; the View menu additionally offers an embedded
  terminal panel), Cut/Copy/Paste,
  Move to Trash, Copy Path, New Folder, New Document (with ~/Templates
  support), Properties. The create dialogs carry an optional,
  collapsed permissions section (rwx matrix plus an editable octal
  field — typing a complete 3-digit value like `755` applies it
  instantly and syncs the checkboxes) — left alone, the system
  defaults apply
- Virtual locations via GVFS: `trash:///`, `recent:///`, `network:///`,
  `smb://`, `sftp://`, `admin://`
- Embedded terminal panel (View → Terminal Panel or F4): a real VTE
  shell docked at the bottom of the window. Spawns the
  user's `$SHELL` in the current folder and follows every navigation
  and tab switch via `cd`; visibility and height are persisted across
  sessions. Remote locations without a local path leave the shell where
  it is.
- Split view (View → Split View or F3): the active tab
  divides horizontally into two fully independent panes — each with its
  own back/forward/up history and breadcrumb bar. Whichever pane was
  last used owns the toolbar, status line, search and zoom. The row
  context menu gains "Copy to Other Pane" / "Move to Other Pane", and
  drag & drop works across the panes. Single pane remains the default.
- Drag & drop: dragging a selected row carries the whole selection;
  the drag cursor is a small card showing the file's icon and name
- Sidebar: Trash offers "Empty Trash…" on right-click; mounted
  devices offer Open / Open in New Tab / Open in New Window /
  Unmount / Eject
- Cloud integration (generic WebDAV via gvfs-dav — Nextcloud,
  ownCloud and any other WebDAV provider): server URL + username in
  Preferences (the app password is stored encrypted in the system
  keyring via libsecret, never in GSettings — the dialog reports the
  status and offers "Forget Password"). A bare host uses the
  Nextcloud/ownCloud endpoint convention
  (/remote.php/dav/files/<user>); a URL with path is used verbatim,
  covering arbitrary WebDAV roots. A cloud row labelled with the
  configured host appears under Network; "Upload to Cloud…" on
  files/folders asks for a cloud target folder (with optional new
  subfolder) and transfers through the regular copy engine, so
  progress, cancel and the conflict dialog apply unchanged
- Synced folders: a local directory can be paired with a cloud folder
  in Preferences (or via a folder's context menu, "Sync with
  Cloud…"). Each pair has its own direction — upload-only
  (local is the source of truth) or two-way mirror — and its own
  schedule: manual, every 5/15/60 minutes, or live on change
  (GFileMonitor, debounced). A per-pair journal under
  `~/.local/share/valkyrie/sync/` records the fingerprints actually
  observed on both sides, so deletions propagate correctly and edits
  on both sides keep the newer version while the loser is preserved
  as "(conflict)" — nothing is silently overwritten. Interval and
  live syncs run quietly in the background; "Sync Now" shows the
  regular progress window
- Right-click on sidebar places (Home, Desktop, Trash, …): Open in
  New Tab, Open in New Window, and — for real filesystem paths —
  Open in Terminal
- Batch rename for multi-selections (Rename… on 2+ files): find/
  replace plus a pattern with {name}/{ext}/{n} placeholders and a
  start-counter, live preview, per-file failure report
- Clipboard staging is visible — going beyond most file managers:
  cut items ghost out instantly (icon + cell fade), copied items
  get a faded icon too ("staged for paste", a marker most file managers do
  not show). Both resolve the moment the paste lands — rows update
  on clipboard change without any manual refresh
- Free space of the current filesystem permanently in the status
  bar (skipped on virtual locations)
- Per-type thumbnail policy (Preferences → Thumbnails): images,
  videos and PDFs each get Always / Only up to N MB / Never —
  oversized files fall back to the plain type icon
- "Set as Background" on image files: writes the desktop GSettings
  background keys; the ragnar daemon watches them and relaunches
  swaybg (toggleable in Context menu settings)
- Editable Properties dialog: the Basic page renames
  directly from the Name entry; the Permissions page offers real
  Owner/Group dropdowns — chown/chgrp that needs privileges
  transparently elevates via pkexec (graphical password prompt), so
  regular users can change owners directly in the dialog
- German and English UI (gettext, English fallback)

## Build

```bash
cargo build --release

# gettext catalogs (needed for the .deb assets)
for lang in de en; do
    mkdir -p target/locale/$lang/LC_MESSAGES
    msgfmt po/$lang.po -o target/locale/$lang/LC_MESSAGES/valkyrie.mo
done
```

## Local install (no package)

```bash
cp target/release/valkyrie ~/.local/bin/

# app icon (hicolor) + desktop file + GSettings schema as usual
mkdir -p ~/.local/share/icons/hicolor/512x512/apps
cp icon/icon.png ~/.local/share/icons/hicolor/512x512/apps/valkyrie.png

# Valkyrie icon theme — picked up automatically at startup
mkdir -p ~/.local/share/icons
cp -r Theme/Valkyrie ~/.local/share/icons/
```

The icon theme is activated via `gtk-icon-theme-name` for Valkyrie's
windows only — other applications keep their configured theme.

## Package

```bash
cargo install cargo-deb   # once
packaging/build-deb.sh
# produces target/debian/valkyrie_<version>-1_amd64.deb
```

`build-deb.sh` builds the release binary, compiles the gettext
catalogs into `target/locale/` (cargo-deb copies files verbatim) and
runs `cargo deb`. The package ships the binary, the desktop entry,
the GSettings schema, the hicolor app icon, the Valkyrie icon theme
and the `de`/`en` catalogs — schema compilation and desktop/icon
caches run through dpkg file triggers on install.

Runtime dependencies beyond GTK4/GLib: `gvfs`, `gvfs-backends`,
`gvfs-daemons` (trash, recent, network, admin locations, WebDAV for
the cloud integration via `gvfsd-dav`) and `libsecret-common` (keyring storage
for the app password).

To register Valkyrie as the default file manager system-wide, set
`inode/directory=org.lieraos.valkyrie.desktop` in the distribution's
`/etc/xdg/mimeapps.list` (or per user in `~/.config/mimeapps.list`).

## Layout

| File | Purpose |
|---|---|
| `src/main.rs` | entry point, i18n init |
| `src/secure_wipe.rs` | secure delete: multi-pass overwrite, medium reliability probe |
| `src/archive.rs` | archive detection, extract (7z/tar), compress (zip/tar.gz/7z) |
| `src/archive_ui.rs` | "Extract Here" and "Compress…" dialogs |
| `src/undo.rs` | undo journal: copy/move/trash reversal (Ctrl+Z) |
| `src/app.rs` | GtkApplication, actions |
| `src/window.rs` | notebook window, actions, shortcuts |
| `src/window/panel.rs` | terminal panel toggle + height persistence wiring |
| `src/terminal_panel.rs` | embedded VTE shell: spawn, `cd` sync, POSIX quoting |
| `src/menubar.rs` | menubar model (File/Edit/View/Help) |
| `src/browser_tab.rs` | one tab: toolbar, pathbar, history, status |
| `src/browser_tab/split.rs` | split-view state, active-pane tracking, sibling registry |
| `src/pane_view.rs` | secondary split pane: own history + breadcrumb |
| `src/browser_tab/status_text.rs` | status line text (count + selection size) |
| `src/browser_tab/scroll_clamp.rs` | GridView scroll-range correction (real content height) |
| `src/browser_tab/close.rs` | tab close interactions: header X button, middle click |
| `src/browser_tab/inflight.rs` | BrowserTab hand-off between windows on tab drag |
| `src/navigation.rs` | back/forward/up, pathbar+entry wiring, activation |
| `src/zoom.rs` | per-directory icon zoom (GVFS metadata) |
| `src/file_view.rs` | view state + public API (options, navigation) |
| `src/dir_watch.rs` | parent-dir monitor: reloads the view when the displayed folder is deleted+recreated (its own monitor dies with the old inode) |
| `src/file_view/selection.rs` | selection queries + change notify |
| `src/view_grid.rs` | icon grid factory (setup/bind recycling) |
| `src/hfw_bin.rs` | height-for-width wrapper: computes the grid's real content height (GtkGridView's constant-size measure otherwise inflates row heights via the viewport allocation) |
| `src/view_widgets.rs` | ColumnView/GridView construction, row menus |
| `src/menu_widgets.rs` | hand-built menu rows (buttons, checks, nested popovers) |
| `src/window_actions.rs` | win.* actions, accelerators, About dialog |
| `src/view_model.rs` | model pipeline: enumerate → map → filter → sort |
| `src/sidebar.rs` | places / devices / network sidebar |
| `src/sidebar/devices.rs` | device rows: mount/unmount/eject, eject button + menu |
| `src/mount_ops.rs` | async volume mount/unmount/eject with error dialogs; opens the existing mount when the automounter already claimed the volume |
| `src/settings_dialog.rs` | preferences window (writes GSettings, applies live) |
| `src/pathbar.rs` | breadcrumb buttons |
| `src/context_menu.rs` | right-click menus (plain GtkPopover, no scrolling) |
| `src/create_entry.rs` | new folder/document dialog + templates |
| `src/operations.rs` | trash + clipboard file ops + creation |
| `src/dnd.rs` | DragSource/DropTarget wiring (rows, background, tab labels) |
| `src/progress.rs` | progress window + message pump (bytes, rate, ETA, verify tally) |
| `src/progress/jobs.rs` | worker-side copy/move/delete orchestration + source filter |
| `src/fast_copy.rs` | local copy engine: FICLONE reflink → streamed copy + inline BLAKE3 |
| `src/copy_pool.rs` | optional worker pool: parallel file-payload copies off a shared FIFO |
| `src/tree_ops.rs` | recursive copy/move/measure/delete + collision resolution |
| `src/tree_ops/verify.rs` | VerifyCtl/VerifyResult types for per-file integrity checking |
| `src/tree_ops/options.rs` | Action/Resolver/ByteCounter/CopyOptions — the tree-ops API surface |
| `src/conflict.rs` | name-conflict dialog (both files' metadata, replace/rename/skip) |
| `src/rename.rs` | rename flow (F2 / menu) via the shared name dialog |
| `src/search.rs` | recursive Ctrl+F search (worker thread + debounce) |
| `src/trash_actions.rs` | restore / delete-forever / empty-trash actions |
| `src/terminal.rs` | terminal emulator detection + spawn |
| `src/thumbnails.rs` | freedesktop thumbnail cache: read + generate for local images (bounded worker pool, texture cache) |
| `src/properties.rs` | properties dialog (Basic page, async dir count) |
| `src/properties_perms.rs` | properties dialog Permissions page (editable owner/group dropdowns, pkexec elevation, rwx matrix + octal entry) |
| `src/mode_editor.rs` | shared permissions editor: rwx checkboxes synced with editable octal field, live 3-digit apply, invalid input marked not erased |
| `src/file_object.rs` | GObject wrapper for directory entries |
| `src/nextcloud.rs` | cloud mount + upload flow (WebDAV/davs://, keyring auth) |
| `src/nextcloud/dialog.rs` | cloud target-folder picker for uploads |
| `src/nextcloud/` | *(internal module names keep the `nextcloud` prefix for config compatibility)* |
| `src/nextcloud/pairs.rs` | sync-pair model + GSettings (de)serialization |
| `src/nextcloud/plan.rs` | pure, unit-tested sync planner → op list |
| `src/nextcloud/exec.rs` | tree listing + op execution via tree_ops |
| `src/nextcloud/state.rs` | per-side sync journal in the user data dir |
| `src/nextcloud/sync.rs` | sync orchestration (mount → list → plan → run) |
| `src/nextcloud/monitor.rs` | interval timers + debounced live watch per pair |
| `src/nextcloud/folders_ui.rs` | preferences list + pair editor dialog |
| `src/secrets.rs` | system keyring wrapper via secret-tool (libsecret) |
| `src/settings.rs` | GSettings access (graceful without schema) |
| `src/i18n.rs` | gettext initialization + `tr()` |

Design notes: plugin/place identity is the `gio::File` URI; applet-style
callbacks use `Rc<RefCell<Vec<Box<dyn Fn>>>` so late subscribers stay safe.
File operations run on worker threads; enumeration is async via
`gtk::DirectoryList`, which also monitors folders for changes.
`src/dir_watch.rs` additionally watches the parent directory so the view
reloads when the displayed folder is deleted and recreated in place —
the DirectoryList monitor is inode-bound and would go silent otherwise.

## License

GPL-3.0-or-later — Copyright (C) 2026 Jörg Schröder, LieraOS Project.


# Deutsch — Änderungen

- **Sicheres Löschen (Einstellungen → Löschen):** Beim endgültigen
  Löschen (Shift+Entf oder Kontextmenü) werden Dateiinhalte wahlweise
  1–7 mal mit Zufallsdaten überschrieben. Beim Aktivieren auf SSDs
  oder Copy-on-Write-Dateisystemen erscheint einmalig ein Hinweis, dass
  das Überschreiben dort nicht garantiert wirkt.
- **Mounten ist Systemaufgabe:** Valkyrie mountet nichts mehr selbst —
  Laufwerke und Wechselmedien bindet die Desktop-Session ein.
  Valkyrie listet Geräte nur noch.
- **Shift+Entf verdrahtet:** das Tastenkürzel löscht jetzt endgültig
  (war zuvor nicht belegt).
- **Kontextmenü anpassbar (Einstellungen → Kontextmenü):** Jeder
  Eintrag außer „Öffnen" und „Eigenschaften" ist einzeln an-/
  abschaltbar — „Im Terminal öffnen" (extern) und „Terminal-Panel
  öffnen" (eingebettetes VTE) sind getrennte Optionen. Auswahl wird
  in GSettings gespeichert und wirkt sofort.
- **Lesezeichen:** Jedes Verzeichnis lässt sich über das Kontextmenü
  („Lesezeichen hinzufügen/entfernen") in der Sidebar anpinnen — die
  Liste wird in GSettings gespeichert und aktualisiert sich live.
- **Archiv-Unterstützung:** „Hier entpacken" auf Archivdateien (zip,
  tar.gz, 7z, rar, xz, bz2 — per Content-Type erkannt) und
  „Komprimieren…" auf jeder Auswahl (zip/tar.gz/7z Format-Auswahl)
  via 7z und tar; beide Einträge sind in den Kontextmenü-Einstellungen
  schaltbar. Der Komprimieren-Dialog füllt den Quellnamen vor;
  Entpacken benennt ein einzelnes Top-Level-Verzeichnis nach dem
  Archivnamen um („Y.zip" → „Y/", nicht der Originalordnername).
- **Eigenschaften-Dialog editierbar:** Der Name im Reiter
  „Grundlegend" benennt die Datei direkt um; unter „Berechtigungen"
  sind Eigentümer und Gruppe als echte Dropdowns wählbar — ein
  chown/chgrp, das Rechte braucht, holt sich die Berechtigung per
  pkexec (grafische Passwortabfrage), sodass auch normale Nutzer den
  Eigentümer wechseln können
- **Rückgängig (Strg+Z):** für Kopieren, Verschieben und
  Papierkorb-Operationen — ein pro-Fenster-Journal zeichnet jede
  umkehrbare Operation auf und stellt den Originalzustand wieder her;
  endgültiges Löschen ist nicht rückgängig machbar.
- **Thumbnails:** für PDF (via pdftoppm/poppler) und Video (via
  ffmpegthumbnailer, falls installiert) zusätzlich zur bestehenden
  Bild-Thumbnail-Cache.
