# Valkyrie TODO

## Open

- [ ] Desktop wallpaper integration decision (ragnar-desktop-view
      already renders desktop icons — Valkyrie must not duplicate)
- [ ] Real Nextcloud/ownCloud verification with TLS (davs://) —
      local WsgiDAV proved plain-dav sync both ways, TLS + Nextcloud
      ETag/chunk semantics still pending on the real server

## Done

- [x] Editable Properties dialog: Name entry on the
      Basic page renames via set_display_name (header follows, error
      reverts + alerts); Permissions page offers real Owner/Group
      dropdowns from /etc/passwd + /etc/group — owner editable for
      everyone, group restricted to own groups for the file owner;
      unprivileged chown/chgrp retries via pkexec (graphical polkit
      prompt, exit 126 = auth dismissed → silent revert)
- [x] "Open as Administrator" actually mounts: the entry navigated to
      admin:// but nobody triggered mount_enclosing_volume — the
      location showed empty. Now mounts via gvfsd-admin + MountOperation
      (polkit prompt through ragnar-polkit-dialog) before navigating,
      on both the item and background context menus
- [x] USB write safety: copies sprinted into the kernel write-back
      cache and reported "done" while gigabytes still drained — pulling
      the stick then corrupted files (ISOs). Now sync_data() every
      32MB paces the writer to device speed (progress stays honest,
      no end-freeze), sync_all() per file + syncfs() per job guarantee
      the data is on the device when the window closes
- [x] Batch rename (src/batch_rename.rs): multi-select → Rename…
      opens a pattern dialog ({name}/{ext}/{n} + find/replace +
      start counter, live preview); single file keeps the prompt
- [x] Visible clipboard staging: cut items ghost (opacity .45),
      copied items carry an accent outline; cleared on paste.
      Rows re-evaluate on every clipboard change via
      FileView::refresh_clip_styles
- [x] Free space permanently in the status bar via
      filesystem::free, appended to the item/selection line
- [x] Per-type thumbnail policy: Preferences → Thumbnails page with
      Always / Up to N MB / Never per category (images, videos,
      PDFs) via thumbnails-*-mode/-max-mb GSettings keys; the pool
      gate skips disallowed items so they keep the type icon
- [x] Set as Background context-menu entry on local image files
      (org.gnome.desktop.background keys — the ragnar daemon
      restarts swaybg on change)
- [x] Clipboard persistence + interop: clipboard.store_async() after
      set_content/set_text (offer survives app exit via a clipboard
      manager — cliphist added to the Liera package list + session
      autostart); text/uri-list uses proper CRLF; gnome-copied-files
      no longer ends in a trailing newline — the empty final entry
      made other file managers fail every paste with "Vorgang wird nicht unterstützt"

- [x] Undo/Redo (Ctrl+Z): per-window journal for copy/move/trash;
      permanent delete is not undoable (`src/undo.rs`)
- [x] Thumbnails for PDF (pdftoppm) and video (ffmpegthumbnailer)
      in addition to image thumbnails


- [x] Bookmarks: sidebar section with live GSettings refresh,
      "Add/Remove Bookmark" context-menu entry (toggleable)
- [x] Archive support: "Extract Here" (7z/tar by extension) and
      "Compress…" (zip/tar.gz/7z chooser) context-menu entries,
      content-type detection, both toggleable in Context menu settings


- [x] Context menu page: per-entry visibility toggles for file/row and
      background menus (`context-menu-hidden` GSettings), new
      "Open Terminal Panel" entry; Open/Properties are always kept


- [x] Secure delete: Preferences → Delete page with overwrite switch,
      1–7 passes spinner and a one-time unreliability warning on
      SSD/COW media; permanent delete wipes file contents with random
      data before unlink (`src/secure_wipe.rs`)
- [x] Shift+Delete bound to delete-permanently (was unwired)
- [x] Automount removed from Valkyrie — mounting volumes is the
      desktop session's job


- [x] Large-directory layout: per-page ScrolledWindow inside the
      view stack (scrollable child is the direct GTK scrollable),
      removing the HfwBin/scroll-clamp workarounds that spread rows
      across a fake inflated height at 1000+ items
- [x] Directory-load speed: icons derived lazily per content type
      (FileObject::icon cache) instead of querying standard::icon for
      every file — ~45ms faster per 5000 files; natural filename sort
      (file_2 < file_10) replaces plain lexical compare


- [x] WebDAV/Cloud sync verified against a live test server:
      sync watches were never installed (anchor resolved before the
      window existed — now resolved at fire time, start moved to
      connect_startup covering activate + CLI open); stale GVFS
      mounts from dead sessions are probed and dropped before reuse


- [x] ISO integration: `packaging/build-deb.sh` builds
      `valkyrie_*.deb` (cargo-deb, assets in Cargo.toml), the Liera
      `build.sh` refreshes it into `config/packages.chroot/` and
      `/etc/xdg/mimeapps.list` makes Valkyrie the default
      `inode/directory` handler on every desktop in the image
- [x] Rust + GTK4 project scaffold (cargo, modular one-purpose files)
- [x] Classic layout: sidebar (places/devices/network), breadcrumb
      pathbar, list/icon views
- [x] Tabs with detach/reorder, per-tab close button and middle-click
      close; tabs dragged outside every window spawn a new window
      (create-window) and carry their BrowserTab across windows
- [x] Context menu: Open, Open as Administrator (admin://), Open in
      Terminal (auto-detect), Cut/Copy/Paste, Copy Path, Trash,
      Properties
- [x] GVFS locations: trash:///, recent:///, network:///
- [x] GVolumeMonitor devices section — mounted volumes whose mount
      GIO marks system-internal (e.g. the live-boot medium at
      /run/live/medium, shadowed/non-user-visible path) now still get
      a device row; previously mount AND volume were both skipped,
      so the device vanished entirely
- [x] gettext de/en, GSettings view-mode + terminal-command keys
- [x] Hidden files filtered by default; Ctrl+H / menu toggle persisted
      (GSettings show-hidden)
- [x] Sorting via "Arrange Items" (name/size/type/modified, reversed),
      persisted (sort-field / sort-descending)
- [x] New Folder / New Document (empty + ~/Templates) with name dialog
- [x] Icon view as default; 64 px themed icons
- [x] Tab bar auto-hidden while a single tab exists
- [x] Zoom slider (status bar), per-directory size via GVFS metadata
      `metadata::valkyrie-icon-size`, GSettings fallback `icon-size`
- [x] Sidebar activation fix (GTK4 rows need activatable=true),
      volume rows mount on click, device order stable across rebuilds
- [x] Context menu restructured into sections ("New" submenu)
- [x] Local install on the productive system (~/.local, absolute Exec)
- [x] Menu bar: File/Edit/View/Help with stateful radio/checkbox
      entries synced against the active tab (MultiSelection views)
- [x] About dialog (version, GPL-3.0, author attribution
      "Jörg Schröder" + GPLv3 §7(b) notice — same convention as
      LieraEdit/LieraEye)
- [x] Context menu scrollbar fix: popovers are now parented to the
      view stack (not the small row box), coordinates translated via
      compute_point
- [x] Popover reuse: one PopoverMenu per view (fixes GTK "broken
      accounting of active state" warning from per-click popovers);
      row menu gained a "Properties" entry
- [x] Popover survives navigation: stack teardown removes only named
      pages, not the parented popover (zombie grab froze the app)
- [x] Context menu never scrolls: rebuilt as plain GtkPopover with
      hand-built rows (menu_widgets.rs) — GtkPopoverMenu embeds a
      scrolled window that could scroll under clamped allocations;
      submenus are real nested popovers (cascade), autohide disabled
      with manual outside-press + Escape dismissal; submenu rows
      unparent their nested popover on unrealize (no finalized-with-
      children warnings)
- [x] Popover opens toward the larger half of the window (GTK only
      flips against the *screen* edge, low clicks got clamped)
- [x] Icon hit areas: GridView stretches cells across the row width;
      row boxes are now centered at natural size, gaps between icons
      are background again
- [x] Window lifetime fix: ValkyrieWindow keeps an Rc cycle until the
      GTK window is destroyed (weak refs in sidebar/actions stayed dead)
- [x] Release build + Xvfb smoke test verified
- [x] Properties dialog rebuilt as a real file-manager dialog:
      icon + name + type header, "Basic" page (type, size or async
      recursive directory contents count, location, modified /
      accessed / created, free space) and "Permissions" page
      (owner/group + read/write/execute matrix applying chmod via
      unix::mode, reverted on backend refusal). Query now includes
      standard::type (fixes GLib-GIO-CRITICAL); notebook minimum
      height covers both pages (fixes GtkWindow measure warnings).
      gio v2_70 feature enabled for access/creation timestamps.
- [x] GtkWindow measure warnings eliminated: value labels ellipsize
      instead of wrap (height no longer width-dependent) and all
      notebook pages are pinned to the tallest page's measured
      natural height — verified warning-free across open, repeated
      tab switches and close.
- [x] Mouse side buttons: 8 = back, 9 = forward, wired on the whole tab
- [x] Thumbnails: freedesktop cache reuse (thumbnail::path) plus
      self-generation for local images via gdk-pixbuf, stored in
      $XDG_CACHE_HOME/thumbnails/large with spec metadata
- [x] View teardown on navigation removed: models are swapped on the
      existing ColumnView/GridView instead of rebuilding widgets —
      no more "broken accounting of active state" when a double click
      triggers the navigation
- [x] New application icon: winged Valkyrie helmet (own identity)
- [x] Sparse-attribute backends: all FileInfo getters go through
      has_attribute guards or raw attribute_* accessors — no GLib
      criticals on network:/// and similar GVFS locations
- [x] "Open in New Tab" + "Open in New Window" context menu entries on
      directories; middle click on a folder opens a new tab; new windows
      are independent Valkyrie windows rooted at the selected directory
- [x] Rename via F2, row context menu and Edit menu — reuses the shared
      name dialog, failures surface as dialogs
- [x] Copy/move/delete progress window on a worker thread with cancel;
      recursive directory copy and post-order delete (gio::File::copy
      is shallow — tree_ops.rs walks the tree); global byte progress;
      same-dir moves and drops into a source's own subtree filtered
- [x] Fast copy engine (fast_copy.rs): FICLONE reflink attempted first
      (instant shared-extents clone on btrfs/xfs), streamed 4 MiB
      read/write pipeline as the universal fallback; metadata is
      re-applied via GIO copy_attributes(ALL_METADATA) on both paths
- [x] Optional copy integrity verification: "Verify integrity"
      checkbox in the progress dialog (persisted via GSettings
      verify-copies, toggleable mid-operation per file). The source
      is hashed inline while its bytes flow through the copy loop —
      no extra source read; the destination is re-hashed while still
      hot in the page cache. Reflink copies skip verification (same
      extents by construction); mismatches are collected and reported
      in one dialog, a clean run gets a confirmation
- [x] Modernized progress dialog: operation icon + title, ellipsized
      current-entry line, percent-labelled bar, detail line with
      bytes done/total + smoothed rate + ETA, live "n/m verified"
      counter during verification
- [x] Parallel copy option (copy_pool.rs + settings page "File
      operations"): copy-parallel/copy-workers GSettings keys; the
      tree walk, directory creation and conflict resolution stay
      sequential while file payloads are pulled from a shared FIFO by
      N workers (UI caps N at the CPU core count, default 4). Pool
      workers run the same reflink/stream + inline-BLAKE3 path and
      account bytes into the shared atomic counter; verify results
      stream back per file. Hint text warns that HDDs/network mounts
      can get slower with parallelism
- [x] Fixed: context-menu Cut/Copy/Copy-Path/Trash/Upload acted only
      on the clicked row even inside a multi-selection, so pasting
      copied just one file. FileView::action_targets() now resolves
      the effective target set (full selection when the picked row is
      part of it, else the row alone) — same rule other file managers
      use
- [x] Progress dialog shows real parallelism: pool workers report the
      in-flight count, so the current-entry label reads
      "file.iso (+1)" while multiple payloads copy at once instead of
      flickering between names
- [x] Trash row menu "Delete Permanently" now honours the selection:
      delete_forever takes the whole action_targets() list instead of
      the clicked file alone
- [x] Grid row spacing is now uniform across folders: GtkGridView
      reports GTK_SIZE_REQUEST_CONSTANT_SIZE, so gtk_widget_measure
      resets for_size to -1 in the request cache — its natural height
      is always computed single-column (n_items rows), the viewport
      allocates that inflated height, and grid allocation then clamps
      every row to allocated_height / 30 = giant gaps. New hfw_bin.rs
      wraps the grid, reports HEIGHT_FOR_WIDTH and computes the real
      content height itself (model count + a realized cell's measured
      size + GTK's own column formula for the real width). Verified
      visually under Xvfb: 334 folders now pack tight and scroll
      correctly. Additionally captions reserve exactly two text lines
      (measured via Pango metrics) with yalign 0, giving every cell an
      identical, stable height; the auto-viewport uses
      hscroll-policy=minimum so the grid never gets a wider-than-
      window allocation
- [x] Automount: automount.rs mounts every can-mount, not-yet-mounted
      volume once at startup and on every volume-added (hot-plug) —
      the live session's boot medium now shows up mounted instead of
      as a greyed sidebar entry. Mount failures stay silent by design
      (the sidebar row remains for manual retry)
- [x] Trash actions: restore via trash::orig-path, permanent delete,
      empty trash — confirmation dialogs for destructive steps
- [x] Recursive search (Ctrl+F + toolbar button): worker-thread
      traversal, streaming results through the existing view pipeline,
      debounced keystrokes cancel previous runs (generation counter —
      no SourceId::remove on fired sources). Traversal stays on the
      starting filesystem (unix::device, find -xdev semantics) so
      /proc, /sys, /run and mounts are skipped; symlinked directories
      are never descended into (NOFOLLOW), which kills merged-usr
      duplicate hits (/bin vs /usr/bin); hits whose attributes can no
      longer be queried are dropped instead of rendering as generic
      files
- [x] Drag & drop: gdk::FileList payload on rows, drop targets on
      folder rows, view background and tab labels; drop = move,
      Ctrl+drop = copy
- [x] Unit tests: unique_destination collisions, source filtering,
      recursive copy/delete, search walk + cancellation,
      trash-location detection — 15 tests, all green
- [x] Name-conflict dialog for copy/move (conflict.rs): existing vs.
      incoming item side by side (icon, size, modified, created) with
      Replace/Merge, Rename (free name entry, validated, slash rejects),
      Skip, Cancel and "apply to all" (never remembered for Rename —
      each file needs its own name); worker parks on a reply channel
      while the modal dialog is up. tree_ops gained move_tree with
      merge semantics, rename recurses re-resolution, kind-mismatch
      guard for remembered Overwrite answers; same-dir paste
      (src == target) auto-duplicates as "name (2)" — that case is a
      duplicate request, not a conflict.
- [x] Rubber-band multi-selection: enable_rubberband on GridView and
      ColumnView; multi-select works with Ctrl/Shift and copy/cut/
      delete operate on the whole selection
- [x] Side-button navigation reworked: buttons 8/9 consumed by an
      EventControllerLegacy in capture phase (GestureClick on 8/9 leaks
      into "any-button" gestures and corrupts GTK sequence bookkeeping),
      navigation idle-deferred
- [x] GTK bug worked around: implicit pointer-focus grab is created on
      every button press but only released for buttons 1-5
      (GdkModifierType has no bits for 6+) — a side-button press leaks
      a grab that misroutes the next click to the stale target.
      Fixed by briefly mapping an invisible helper popover whose grab
      revokes stale implicit grabs (gtkwindowgroup revoke path)
- [x] 18 tests green (conflict rename/skip/merge coverage added)
- [x] Device mount/unmount/eject: mounted volumes get an eject button
      and a right-click menu (Open/Unmount/Eject), unmounted volumes
      offer Mount; mount_ops.rs runs the async ops through
      MountOperation and reports failures via AlertDialog. Sidebar
      rebuilds explicitly after each op (daemon mounts don't reliably
      emit mount-removed). Sidebar split: devices live in
      sidebar/devices.rs.
- [x] Clickable column headers: each column carries a field sorter,
      the model chains directories-first + the ColumnViewSorter via
      MultiSorter, so header clicks and menu actions share one state
      (primary_sort_column/order); sort persists to GSettings on every
      change and seeds once at startup
- [x] Preferences dialog (Edit → Preferences, Ctrl+,): default view,
      hidden files, default icon size, terminal command override —
      live-applied to all open tabs where it makes sense
- [x] Preferences expanded into sections (View / Sorting / Terminal);
      the terminal row is now a picker listing every emulator detected
      on PATH plus "Automatic detection"
- [x] Status bar reports selection: "N items — M items selected
      (size)" via trn() plurals + glib::format_size; selection_summary
      in file_view/selection.rs counts objects, skips dir recursion
- [x] System clipboard interop: set_clipboard publishes
      x-special/gnome-copied-files (with cut/copy leader) +
      text/uri-list via a union ContentProvider; paste() reads the
      system clipboard async so lists copied in other apps/desktop
      shells paste into Valkyrie (verified cross-process under Xvfb)
- [x] Icon-view selection shape: app stylesheet (data/valkyrie.css,
      embedded at startup) turns the stretched grid-cell highlight
      transparent and paints a rounded accent box on the item box
      (.file-cell.selected, toggled from ListItem::selected); theme's
      12px cell margin reduced to 4px. GTK node is "child" (verified
      against the libgtk theme), @accent vars unavailable on plain
      GTK4 — selection accent hardcoded to the theme value. Selection
      mirrored between the two MultiSelection models on view switch
- [x] Icon cards & dead-zone clicks: item box now fills its grid cell
      (uniform card, hexpand+fill) so a click beside an icon can no
      longer land on a neighbor; the ~4px inter-cell gap plus all
      background area clears the selection via a button-1 gesture
      that denies the sequence (rubber band untouched). Long names
      wrap word-char at max 2 lines + ellipsize (display only, real
      name kept, tooltip shows it)
- [x] Zoom persistence fixed: GVFS metadata only accepts strings —
      attribute_int32 wrote nothing; now stored as decimal string and
      parsed back (verified across app restart)
- [x] Valkyrie identity: icon/icon.png installed as
      hicolor/512x512/apps/valkyrie.png (SVG fallback kept); bundled
      Theme/Valkyrie activated at startup via gtk-icon-theme-name when
      installed under an XDG icon dir (per-user or system), inheriting
      Adwaita/hicolor. Old placeholder SVG removed — icon.png is the
      single source (hicolor/512x512 + deb asset)
- [x] Icon view gaps are real: GTK resolves clicks via the nearest
      tile, so only border-spacing between tiles is true dead area —
      grid border-spacing 8px keeps the strips between cards clickable
      background (selection clears, no neighbor activates) without
      looking bloated
- [x] Oversized item spacing fixed: the stock theme gives every grid
      child box an invisible 12px margin + 8px border-spacing and the
      cell itself 3px padding — that padded every card ~30px wider,
      collapsed the column count and spread items far apart (worst in
      folders with a saved higher zoom like Downloads, because
      GtkGridView divides the window width evenly by the column count:
      card 124px → 5 columns → ~40px dead slack per tile). The CSS now
      resets cell margin/padding and card margin/border-spacing, and
      the card request is px+14 — just the icon plus its inner
      padding. Downloads (zoom 104) now packs 6 columns at ~136px
      pitch, matching the home directory's density
- [x] Scroll range fixed: GtkGridView lacks get_request_mode, so GTK
      treats it as CONSTANT_SIZE and measures its height at the
      min_columns layout for every request — the scrolled window
      allocated a hugely oversized content height. scroll_clamp
      recomputes the real height from realized cells (column count
      from first-row cells, pitch from row delta) and clamps the
      vadjustment upper back down — verified under Xvfb
- [x] Optional permissions on create: New Folder/Document dialogs got a
      collapsed rwx expander (owner/group/others matrix, live octal in
      the label) preset to 0755/0644 — collapsed keeps the umask
      defaults, expanded applies the bits via unix::mode after create.
      Verified end-to-end (0744 applied), unit-tested
- [x] Zoom readout: tooltip on the slider carries the current icon
      size ("Zoom: N", updated live) — a permanent readout had made
      the status bar too tall; range 16–300, schema + settings updated
- [x] Icon aspect fixed: the card's fixed height request squeezed
      image + caption into too little space, flattening the icon —
      height is natural now (uniform card width kept), folders look
      right at every zoom
- [x] Default icon size raised 48 → 80 (GSettings schema + fallback;
      per-directory GVFS zoom still wins where already saved)
- [x] Nextcloud integration: server URL + username in Preferences
      (GSettings nextcloud-server/-user; http(s):// input normalized),
      app password stored encrypted in the system keyring via
      secret-tool/libsecret with visible status + "Forget Password"
      (secrets.rs spawns secret-tool, password on stdin — never in
      argv, GSettings or code). Sidebar "Nextcloud" row under Network
      mounts davs://<host>/remote.php/dav/files/<user>/ with a
      MountOperation fed from the keyring and browses it;
      settings changes refresh the row live. Context menu "Upload to
      Nextcloud…" opens a target-folder picker (nextcloud/dialog.rs —
      enumerates the remote root on a worker thread through a channel
      + main-context poll, optional new subfolder created before
      copy) and pipes through progress::copy_move, so progress,
      cancel and the name-conflict dialog apply unchanged. Row stays
      hidden until server + user are configured. 35 tests green.
- [x] Synced folders (permanent local↔Nextcloud pairs): each pair is a
      local dir + remote path + direction (upload-only mirror or
      two-way) + schedule (manual / 5 / 15 / 60 min / live on change).
      Stored in GSettings nextcloud-sync-folders (a(sssi)); managed in
      Preferences → Nextcloud (folders_ui.rs: list + add dialog with
      folder picker, direction and schedule dropdowns; remove also
      drops the journal). Context menu "Sync with Nextcloud…" on a
      local directory opens the pair dialog prefilled, or runs
      "Sync Now" on an existing pair. Engine: pure plan.rs (12 unit
      tests) maps local/remote listings + the journal onto an op list
      (Upload/Download/DeleteLocal/DeleteRemote/ConflictUpload/
      ConflictDownload); exec.rs executes via tree_ops; sync.rs
      mounts, lists, plans, runs and journals; monitor.rs drives
      interval timers and debounced GFileMonitor live watches, torn
      down and rebuilt on every settings change.
      Journal (state.rs): per-side (mtime,size) fingerprints re-read
      from BOTH trees after the ops — WebDAV doesn't preserve upload
      mtimes, so journaled expectations would re-conflict forever.
      Both-sides edits keep the newer file under the name, the loser
      becomes "<name> (conflict)"; deletes propagate only for paths
      journaled on both sides. 52 tests green.
- [x] Sidebar places context menu: right-click on Home/Desktop/
      Recent/Filesystem/Trash offers Open in New Tab / Open in New
      Window, plus Open in Terminal where the target is a real path;
      verified end-to-end under Xvfb (new tab opened on Home).
- [x] GTK-Measure-Warnung behoben: die Ansichts-Stack hatte
      hhomogeneous/vhomogeneous=true (Default) → GTK maß auch das
      unsichtbare ColumnView (Mindestbreite 640) transient mit
      Breite 0 → "Trying to measure ... needs at least 640". Stack
      jetzt nicht-homogen: nur das sichtbare Kind wird gemessen.
      (Lazy-Listenansicht als Alternative verworfen — sie hätte die
      geteilte Sortier-Pipeline ohne ColumnView-Sorter still gelegt.)
- [x] Open With: file context menus carry an "Open With" submenu fed
      by gio::AppInfo::recommended_for_type (app icons + names, files
      only, never in trash). "Other Application…" opens a filterable
      chooser of all installed apps with an optional "Always use for
      this file type" checkbox (set_as_default_for_type +
      set_as_last_used_for_type). Verified under Xvfb: PNG submenu
      listed 6 registered apps, clicking GIMP launched it with the
      file, the chooser dialog opens.
- [x] Preferences redesigned as category navigation: GtkStackSidebar
      on the left (View / Sorting / Terminal / Nextcloud), the active
      page on the right, each page in its own ScrolledWindow — the
      dialog stays compact no matter how many sections grow.
      Verified under Xvfb (sidebar switching works, pages render).
- [x] "Broken accounting of active state" warnings fixed: the row,
      background and places menus opened their popovers on button
      PRESS; when the pointer moved before release, GTK's
      active-state accounting broke along the pressed widget's
      ancestry (GTK bug #4563, fixed upstream in GTK 4.21.6 — this
      system runs 4.18.6). All right-click gestures now pop up on
      RELEASE, the X11 menu convention (devices.rs already did);
      places keep claiming the sequence on press so other row
      handlers stay suppressed. Verified under Xvfb: all three
      menus open reliably after press-move-release, zero warnings.
- [x] Rendering/scrolling performance pass: list factories now follow
      the GTK4 recycling contract — widget construction in
      connect_setup, data-only updates in connect_bind (previously a
      full widget tree was rebuilt on every bind). Row gestures and
      DnD resolve item.item() at event time so recycled rows act on
      the currently bound object. Selection-css handler is connected
      once in setup instead of once per bind (fixed a per-scroll-step
      handler leak). Grid factory moved to src/view_grid.rs.
- [x] Thumbnail pipeline rewritten: bounded 4-worker pool on a shared
      mpsc queue instead of one OS thread + one 50 ms timer per bound
      item; results return via thread-safe glib::idle_add_once; a
      per-image generation counter prevents stale writes onto
      recycled cells; decoded textures cached in a 512-entry FIFO so
      scrolling back is instant.
- [x] Sort comparator zero-alloc: lowercase sort key + dotfile flag
      cached per FileObject at construction (was: two fresh
      lowercase strings per comparison — ~130k allocs for 10k files).
- [x] "Other Application…" chooser launched the wrong app: it looked
      up the selected row via ListBoxRow::index(), which is the
      *visible* position — after sorting/filtering that no longer
      matched the AppInfo list order, so an unrelated app started.
      Rows now carry an explicit (row, app) association; verified
      end-to-end under Xvfb with a marker desktop file (filtered
      index 0, correct app launched).
- [x] Cloud support generalized from Nextcloud-only to any WebDAV
      provider: the server field accepts a bare host (Nextcloud/
      ownCloud convention /remote.php/dav/files/<user> is assumed)
      or a full URL whose path is used verbatim as the DAV endpoint
      ({user} placeholder expands to the login; path + userinfo are
      URI-escaped, so email-style logins work). User-facing strings
      now say "Cloud"/"WebDAV" (settings page, context menu, upload
      and pair dialogs, keyring label); the sidebar row shows the
      configured host. Internal GSettings keys keep the nextcloud-*
      prefix so existing configs survive. 4 new parse tests (57 total).
- [x] Octal permission entry: a shared `mode_editor` module renders
      the rwx matrix plus an editable octal field, synced in both
      directions — typing "755"/"777" updates the checkboxes, toggles
      update the field; commits happen on Enter/focus-out, invalid or
      special-bit input (1777) is rejected with error styling instead
      of silently masked. Used by the properties "Permissions" page
      (immediate chmod; bits outside the nine rwx positions — type,
      setuid/setgid/sticky — are preserved on write, failure reverts
      widgets) and by the New Folder/Document expander. 3 new parse/
      format tests (61 total).
- [x] Octal entry applies live: a complete 3-digit octal value commits
      on the keystroke that completes it (checkboxes + chmod follow
      immediately); fragments stay pending until Enter/focus-out so
      intermediates like "7" cannot flip permissions, and invalid
      input marks the field without wiping the text mid-typing.
- [x] Embedded terminal panel (): View → "Terminal Panel"
      or F4 docks a real VTE shell at the bottom of the window — separate
      from the context menu's "Open in Terminal", which still opens a
      standalone window. The shell spawns lazily in the active tab's
      folder and follows every navigation/tab switch via `cd` (POSIX
      single-quote escaping); remote GVFS locations without a local path
      are skipped. Visibility + panel height persist via GSettings
      (terminal-panel-visible, terminal-panel-height); the header's close
      button toggles the same win.terminal-panel action. 4 new
      shell_quote tests (76 total); verified under Xvfb (spawn cwd,
      keyboard input, cd sync on navigation).
- [x] Split view (, opt-in): View → "Split View" or F3
      divides the active tab into two independent panes (PaneView with
      own back/forward/up history + breadcrumb). The last-used pane
      owns toolbar/search/zoom/status; row context menus gain "Copy/Move
      to Other Pane" resolved through a per-view sibling registry.
      BrowserTab's split state lives in Rc-shared cells so clones made
      by action handlers observe the same pane (fixed: activate in the
      secondary pane dead-referenced the dropped Rc). Verified under
      Xvfb: F3 split renders both panes, double-click navigates only
      the focused pane.
- [x] Directory monitoring survives delete+recreate: GtkDirectoryList's
      internal monitor is bound to the inode it started on — deleting
      and recreating the displayed directory (lb build/clean on
      chroot/, binary/, cache/; git checkouts; atomic dir swaps) killed
      the watch silently and froze the view at its last contents until
      F5. New dir_watch module monitors the *parent* directory and
      reloads the view when the folder reappears (Created/MovedIn);
      the widget's destroy handler breaks the monitor→callback→view
      reference cycle. 2 new tests (78 total); verified under Xvfb —
      files created in a recreated dir now appear automatically.
- [x] Sidebar mount race with the automounter: clicking a device row
      while the automounter was still mounting it (first window after
      startup) failed with "already mounted" / a mount error dialog,
      and a stale GVolume object could keep reporting no mount at all.
      mount_and_open now resolves the live mount first (via the
      volume's unix-device identifier against the monitor's mount
      list, not only get_mount()), navigates into it directly, and on
      a failed mount request waits up to 3s for the automounter's
      in-flight mount before showing an error.
