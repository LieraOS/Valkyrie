# Valkyrie TODO

## Offen

- [ ] Entscheidung Desktop-Hintergrundbild-Integration (ragnar-desktop-view
      rendert bereits Desktop-Icons — Valkyrie darf das nicht doppeln)
- [ ] Echte Nextcloud/ownCloud-Verifikation mit TLS (davs://) —
      lokaler WsgiDAV bewies Plain-DAV-Sync in beide Richtungen, TLS +
      Nextcloud-ETag-/Chunk-Semantik steht am echten Server noch aus
## Erledigt

- [x] Veraltete Thumbnails nach Überschreiben: GIO meldet
      thumbnail::path auch für Einträge, deren eingebettetes
      Thumb::MTime nicht mehr zur Datei passt — Valkyrie hat blind
      vertraut und nie neu generiert. Worker validieren jetzt das
      tEXt Thumb::MTime der PNG gegen die echte mtime und
      regenerieren bei Abweichung (src/thumbnails/generate.rs); der
      In-Memory-Textur-Cache keyed auf uri+mtime, damit ein
      Überschreiben nie das alte Bild neu zeichnet. Bonus gefixt:
      generate_pdf löschte sein Temp-PNG vor dem Lesen — PDFs bekamen
      gar keine Thumbnails
- [x] USB-Kopie wirkt nicht mehr eingefroren: Der Balken zeigte
      Bytes, die der Page-Cache übernommen hatte (Sprint → Vollstopp
      beim Throttlen → kleine Schritte). Im paced-Modus (Wechsel-
      datenträger) meldet der Fortschritt jetzt die vom Gerät
      quittierten Bytes aus dem sysfs-Write-Counter — innerhalb der
      Throttle-Schleife UND in einer Drain-Phase nach dem Schreib-
      Loop, sodass der Balken kontinuierlich mit echter Schreib-
      geschwindigkeit läuft und das Ende nicht bei fast-voll parkt
- [x] Alle Klicks tot nach Maus-Seitentasten (Zurück/Vor): GTK legt
      bei JEDEM Button-Press einen impliziten Pointer-Focus-Grab an,
      räumt ihn aber nur bei Release von Button 1-5 auf — jeder
      8/9-Klick leakt einen Grab, der danach alle normalen Klicks
      schluckt (Hover läuft weiter, Zurück/Vor auch, weil sie am
      Grab vorbei gehen). Der alte Workaround (unsichtbares Popover
      am Tab-Widget) versagte, sobald das stale Grab-Target ein
      VORFAHR des Popovers war — z. B. der Paned-Divider, der
      Tab-Container selbst, alles darüber — und ein Press auf ein
      Sidebar-Level-Target ließ den tab-scoped Controller blind
      werden. Jetzt lebt der Seitentasten-Controller am Toplevel
      (sieht 8/9-Events egal wohin der Grab retargetet) und das
      Revoke-Popover hängt am Fenster — nur ein stale Target direkt
      am Window-Widget könnte ihn noch schlagen, was Picking
      praktisch nie liefert. Verifiziert unter Xvfb: Button-8 auf
      dem Paned-Divider fror die alte Binary komplett ein; der
      gefixte Build liefert Klicks weiterhin
- [x] Flackern + Absturz bei Update-Stürmen: Der file_view-Monitor
      tauschte bei jedem debounced Event die komplette
      GtkDirectoryList — sichtbares Neu-Aufploppen aller Zeilen und
      bei Datei-Flut (Kernel-Build) Main-Loop-Verhungern. Ersetzt
      durch src/dir_model: eigener ListStore + FileMonitor,
      Changed/Created/Deleted/Moved-Events aktualisieren einzelne
      Zeilen per async query_info (Trailer-Timer pro Pfad, >64
      anstehende Pfade eskalieren), strukturelle Events debouncen in
      eine async Re-Enumeration + minimale Splice-Diffs — kein
      Flackern, Selektion und Scroll-Position überleben
- [x] Kopieren zwischen Fenstern blockierte beide Fenster: zwei
      ineinandergreifende Ursachen — das Fortschrittsfenster war
      .modal(true) (labwc grabbt den Fokus für die Transient-Gruppe)
      und, der eigentliche Blocker, der Changed-Event →
      Komplett-Rebuild-Sturm pro Kopiervorgang ließ den Main-Loop
      verhungern. Modal entfernt; der Rebuild-Sturm ist mit DirModel
      (s. o.) weg
- [x] Kompaktansicht: kein Kontextmenü bei Rechtsklick ins Leere —
      attach_background_menu + attach_empty_click_clear fehlten auf
      dem kompakten ColumnView schlicht; jetzt wie Listen-/Symbol-
      ansicht verdrahtet
- [x] org.freedesktop.FileManager1-D-Bus-Interface implementiert
      (src/filemanager1.rs): besitzt den bekannten Namen auf dem
      Session-Bus (laufende Instanz gewinnt gegenüber Aktivierung),
      exportiert ShowItems (öffnet das Elternverzeichnis + selektiert
      den Eintrag über eine Pending-Selektion, die greift sobald die
      Enumeration die Zeile liefert), ShowFolders (Fenster + Tabs),
      ShowItemProperties. Liefert
      data/org.freedesktop.FileManager1.service mit (Exec=valkyrie
      --gapplication-service, wird gehalten solange es den Namen
      besitzt) — „Datei anzeigen" in Firefox erreicht jetzt Valkyrie
- [x] Terminal-Panel cd-Injection: Das Panel folgte der Navigation,
      indem es `cd` in die Eingabe der laufenden Shell fütterte —
      bei halb getipptem Befehl am Prompt landete das injizierte
      `cd ` mitten in der Zeile oder führte die Teileingabe aus.
      Behoben über Prompt-State-Awareness: VTE-`commit`-Bytes merken
      sich, ob die Editiereile „schmutzig" ist (Enter/Strg+C/Strg+U
      räumen sie); eine Navigation bei schmutziger Zeile (oder
      solange ein Vordergrund-Child wie vim die PTY hält, geprüft
      via tcgetpgrp gegen die Shell-PID) reiht das Ziel in eine
      Warteschlange, und das gequeue `cd` wird beim nächsten
      zeilenleerenden Commit geflusht, sobald die Shell wieder den
      Vordergrund hält
- [x] Kontextmenü überlebt Fenster-Fokuswechsel: Rechtsklick-Popovers
      (Dateiansicht und Sidebar-Geräte) hatten autohide aus und
      überwachten nur Klicks aufs eigene Fenster — ein Menü in
      Fenster A schwebte über aktiviertem Fenster B. Beide poppen
      jetzt bei is-active-Verlust des eigenen Fensters zu

- [x] Verzeichnisansicht übernimmt extern angelegte Dateien: per Test
      reproduziert — GtkDirectoryLists interner Monitor verwirft
      ins-Verzeichnis-verschobene Dateien komplett (Browser
      Temp→Rename-Downloads, mv, atomare Saves; einfache Creates kamen
      noch an). Behoben über einen eigenen debounced GFileMonitor auf
      dem angezeigten Ordner, der eine frische DirectoryList unter
      das bestehende MapListModel tauscht — Filter/Sortierung/
      Selektionen/Widgets unangetastet

- [x] Editierbarer Eigenschaften-Dialog: Namensfeld auf der Seite
      „Grundlegend" benennt via set_display_name um (Header folgt,
      Fehler revertiert + meldet); die Seite „Berechtigungen" bietet
      echte Eigentümer/Gruppen-Dropdowns aus /etc/passwd +
      /etc/group — Eigentümer für jeden änderbar, Gruppe auf eigene
      Gruppen beschränkt für den Dateieigentümer; unprivilegiertes
      chown/chgrp versucht es via pkexec erneut (grafische Polkit-
      Abfrage, Exit 126 = Auth abgebrochen → stilles Revert)
- [x] „Als Administrator öffnen" mountet tatsächlich: der Eintrag
      navigierte zu admin://, aber niemand löste
      mount_enclosing_volume aus — der Ort zeigte leer. Jetzt Mount
      via gvfsd-admin + MountOperation (Polkit-Abfrage über
      ragnar-polkit-dialog) vor dem Navigieren, sowohl im Zeilen- als
      auch im Hintergrund-Kontextmenü
- [x] USB-Schreibsicherheit: Kopien sprinteten in den Kernel-Write-
      Back-Cache und meldeten „fertig", während noch Gigabytes
      abflossen — Stick ziehen korrumpierte dann Dateien (ISOs).
      Jetzt paced sync_data() alle 32MB den Writer auf
      Gerätegeschwindigkeit (Fortschritt bleibt ehrlich, kein End-
      Freeze), sync_all() pro Datei + syncfs() pro Job garantieren,
      dass die Daten auf dem Gerät sind, wenn das Fenster schließt
- [x] Stapel-Umbenennen (src/batch_rename.rs): Mehrfachauswahl →
      Umbenennen… öffnet einen Muster-Dialog ({name}/{ext}/{n} +
      Suchen/Ersetzen + Startzähler, Live-Vorschau); Einzeldatei
      behält den Prompt
- [x] Sichtbare Zwischenablage-Bereitstellung: ausgeschnittene
      Einträge ghosten (Opacity .45), kopierte tragen einen Accent-
      Rahmen; wird beim Einfügen aufgelöst. Zeilen werden bei jeder
      Zwischenablage-Änderung neu bewertet via
      FileView::refresh_clip_styles
- [x] Freier Speicher dauerhaft in der Statusleiste via
      filesystem::free, an die Eintrags-/Auswahlzeile angehängt
- [x] Per-Typ-Thumbnail-Richtlinie: Einstellungen → Thumbnails-Seite
      mit Immer / Bis zu N MB / Nie pro Kategorie (Bilder, Videos,
      PDFs) via thumbnails-*-mode/-max-mb GSettings-Keys; das Pool-
      Gate überspringt nicht erlaubte Einträge, sodass sie das
      Typ-Icon behalten
- [x] „Als Hintergrundbild setzen"-Kontextmenü-Eintrag auf lokalen
      Bilddateien (org.gnome.desktop.background-Keys — der Ragnar-
      Daemon startet swaybg bei Änderung neu)
- [x] Zwischenablage-Persistenz + Interop: clipboard.store_async()
      nach set_content/set_text (Angebot überlebt App-Ende via
      Clipboard-Manager — cliphist in der Liera-Paketliste + Session-
      Autostart ergänzt); text/uri-list nutzt korrektes CRLF;
      gnome-copied-files endet nicht mehr mit einem trailing
      Newline — der leere letzte Eintrag ließ andere Dateimanager
      bei jedem Einfügen mit „Vorgang wird nicht unterstützt"
      scheitern

- [x] Rückgängig/Wiederholen (Strg+Z): Journal pro Fenster für
      Kopieren/Verschieben/Papierkorb; endgültiges Löschen ist nicht
      rückgängig machbar (`src/undo.rs`)
- [x] Thumbnails für PDF (pdftoppm) und Video (ffmpegthumbnailer)
      zusätzlich zu Bild-Thumbnails


- [x] Lesezeichen: Sidebar-Sektion mit live GSettings-Refresh,
      „Lesezeichen hinzufügen/entfernen"-Kontextmenü-Eintrag
      (schaltbar)
- [x] Archiv-Unterstützung: „Hier entpacken" (7z/tar nach Endung) und
      „Komprimieren…" (zip/tar.gz/7z-Auswahl) Kontextmenü-Einträge,
      Content-Type-Erkennung, beide in den Kontextmenü-Einstellungen
      schaltbar


- [x] Kontextmenü-Seite: Per-Eintrag-Sichtbarkeits-Schalter für
      Datei-/Zeilen- und Hintergrund-Menüs (`context-menu-hidden`
      GSettings), neuer „Terminal-Panel öffnen"-Eintrag; Öffnen/
      Eigenschaften bleiben immer


- [x] Sicheres Löschen: Einstellungen → Löschen-Seite mit
      Überschreib-Schalter, 1–7-Durchgänge-Spinner und einmaliger
      Unzuverlässigkeits-Warnung auf SSD/CoW-Medien; endgültiges
      Löschen überschreibt Dateiinhalte mit Zufallsdaten vor dem
      Unlink (`src/secure_wipe.rs`)
- [x] Umschalt+Entf auf delete-permanently verdrahtet (war unbelegt)
- [x] Automount aus Valkyrie entfernt — Laufwerke einhängen ist
      Aufgabe der Desktop-Session


- [x] Großes-Verzeichnis-Layout: Per-Page-ScrolledWindow im
      Ansichts-Stack (scrollbarer Kind-Widget ist das direkte GTK-
      Scrollable), entfernt die HfwBin/Scroll-Clamp-Workarounds, die
      Zeilen über eine aufgeblähte Fake-Höhe bei 1000+ Einträgen
      verteilten
- [x] Verzeichnis-Ladegeschwindigkeit: Icons werden lazy pro
      Content-Type abgeleitet (FileObject::icon-Cache) statt
      standard::icon für jede Datei abzufragen — ~45ms schneller pro
      5000 Dateien; natürliche Dateinamen-Sortierung
      (file_2 < file_10) ersetzt reinen Lexik-Vergleich


- [x] WebDAV/Cloud-Sync gegen einen Live-Testserver verifiziert:
      Sync-Watches wurden nie installiert (Anker wurde aufgelöst,
      bevor das Fenster existierte — wird jetzt zur Feuerzeit
      aufgelöst, Start nach connect_startup verlegt, deckt activate +
      CLI-open ab); stale GVFS-Mounts aus toten Sessions werden
      geprüft und vor Wiederverwendung verworfen


- [x] ISO-Integration: `packaging/build-deb.sh` baut
      `valkyrie_*.deb` (cargo-deb, Assets in Cargo.toml), die Liera-
      `build.sh` frischt es in `config/packages.chroot/` auf und
      `/etc/xdg/mimeapps.list` macht Valkyrie zum Standard-
      `inode/directory`-Handler auf jedem Desktop im Image
- [x] Rust + GTK4 Projektgerüst (cargo, modulare Ein-Zweck-Dateien)
- [x] Klassisches Layout: Sidebar (Orte/Geräte/Netzwerk), Breadcrumb-
      Pfadleiste, Listen-/Icon-Ansichten
- [x] Tabs mit Ablösen/Umordnen, Schließen-Button pro Tab und
      Mittelklick-Schließen; Tabs, die aus jedem Fenster gezogen
      werden, spawnen ein neues Fenster (create-window) und tragen
      ihren BrowserTab fensterübergreifend
- [x] Kontextmenü: Öffnen, Als Administrator öffnen (admin://), Im
      Terminal öffnen (Auto-Erkennung), Ausschneiden/Kopieren/
      Einfügen, Pfad kopieren, Papierkorb, Eigenschaften
- [x] GVFS-Orte: trash:///, recent:///, network:///
- [x] GVolumeMonitor-Geräte-Sektion — gemountete Laufwerke, deren
      Mount GIO als systemintern markiert (z.B. das Live-Boot-Medium
      unter /run/live/medium, verdeckter/nicht-nutzer-sichtbarer
      Pfad), bekommen jetzt trotzdem eine Gerätezeile; vorher wurden
      Mount UND Volume übersprungen, sodass das Gerät komplett
      verschwand
- [x] gettext de/en, GSettings view-mode + terminal-command Keys
- [x] Versteckte Dateien standardmäßig gefiltert; Strg+H /
      Menü-Toggle persistiert (GSettings show-hidden)
- [x] Sortierung via „Elemente anordnen" (Name/Größe/Typ/Geändert,
      umgekehrt), persistiert (sort-field / sort-descending)
- [x] Neuer Ordner / Neues Dokument (leer + ~/Vorlagen) mit
      Namensdialog
- [x] Icon-Ansicht als Standard; 64px-Theme-Icons
- [x] Tableiste auto-ausgeblendet, solange nur ein Tab existiert
- [x] Zoom-Regler (Statusleiste), Per-Verzeichnis-Größe persistiert in
      ~/.config/ragnar-settings/valkyrie-zoom.ini — GVFS-Metadaten
      entfernt: gvfsd-metadata kann unter Schreiblast spinnen und
      blockiert dann alle synchronen Metadata-D-Bus-Aufrufe.
      GSettings-Fallback `icon-size`
- [x] Sidebar-Aktivierungs-Fix (GTK4-Zeilen brauchen
      activatable=true), Volume-Zeilen mounten bei Klick, Geräte-
      Reihenfolge stabil über Rebuilds
- [x] Kontextmenü in Sektionen restrukturiert („Neu"-Untermenü)
- [x] Lokale Installation auf dem Produktivsystem (~/.local, absoluter
      Exec)
- [x] Menüleiste: Datei/Bearbeiten/Ansicht/Hilfe mit zustandsbehafteten
      Radio-/Checkbox-Einträgen, gegen den aktiven Tab synchronisiert
      (MultiSelection-Ansichten)
- [x] Über-Dialog (Version, GPL-3.0, Autoren-Nennung „Jörg Schröder" +
      GPLv3-§7(b)-Vermerk — gleiche Konvention wie LieraEdit/LieraEye)
- [x] Kontextmenü-Scrollbar-Fix: Popovers sind jetzt am Ansichts-
      Stack ge parentet (nicht an der kleinen Zeilen-Box),
      Koordinaten via compute_point übersetzt
- [x] Popover-Wiederverwendung: ein PopoverMenu pro Ansicht (behebt
      die GTK-Warnung „broken accounting of active state" von Per-
      Klick-Popovers); Zeilenmenü bekam einen „Eigenschaften"-
      Eintrag
- [x] Popover überlebt Navigation: Stack-Teardown entfernt nur
      benannte Seiten, nicht das geparentete Popover (Zombie-Grab
      fror die App ein)
- [x] Kontextmenü scrollt nie: neu gebaut als schlichtes GtkPopover
      mit handgebauten Zeilen (menu_widgets.rs) — GtkPopoverMenu
      bettet ein gescrolltes Fenster ein, das unter geklemmten
      Allokationen scrollen konnte; Untermenüs sind echte verschachtelte
      Popovers (Kaskade), Autohide deaktiviert mit manuellem
      Außerhalb-Klick + Escape-Schließen; Untermenü-Zeilen
      entparenten ihr verschachteltes Popover bei unrealize (keine
      finalized-with-children-Warnungen)
- [x] Popover öffnet zur größeren Fensterhälfte (GTK flippt nur gegen
      die *Bildschirm*-Kante, tiefe Klicks wurden geklemmt)
- [x] Icon-Hit-Areas: GridView streckt Zellen über die Zeilenbreite;
      Zeilen-Boxen sind jetzt zentriert in natürlicher Größe, Lücken
      zwischen Icons sind wieder Hintergrund
- [x] Fenster-Lebensdauer-Fix: ValkyrieWindow hält einen Rc-Zyklus,
      bis das GTK-Fenster zerstört ist (Weak-Refs in Sidebar/Actions
      blieben tot)
- [x] Release-Build + Xvfb-Smoke-Test verifiziert
- [x] Eigenschaften-Dialog als echter Dateimanager-Dialog neu gebaut:
      Icon + Name + Typ-Header, Seite „Grundlegend" (Typ, Größe oder
      asynchron rekursive Verzeichnisinhalte-Zählung, Ort,
      geändert/zugegriffen/erstellt, freier Platz) und Seite
      „Berechtigungen" (Eigentümer/Gruppe + Lese-/Schreib-/
      Ausführ-Matrix, die chmod via unix::mode anwendet, bei Backend-
      Verweigerung revertiert). Query enthält jetzt standard::type
      (behebt GLib-GIO-CRITICAL); Notebook-Mindesthöhe deckt beide
      Seiten (behebt GtkWindow-measure-Warnungen). gio v2_70-Feature
      für Zugriffs-/Erstellungs-Zeitstempel aktiviert.
- [x] GtkWindow-measure-Warnungen beseitigt: Wert-Labels ellipsizen
      statt umbrechen (Höhe nicht mehr breitenabhängig) und alle
      Notebook-Seiten sind auf die gemessene natürliche Höhe der
      höchsten Seite gepinnt — verifiziert warnungsfrei über Öffnen,
      wiederholte Tab-Wechsel und Schließen.
- [x] Maus-Seitentasten: 8 = zurück, 9 = vorwärts, auf dem ganzen Tab
      verdrahtet
- [x] Thumbnails: freedesktop-Cache-Wiederverwendung (thumbnail::path)
      plus Selbst-Generierung für lokale Bilder via gdk-pixbuf,
      gespeichert in $XDG_CACHE_HOME/thumbnails/large mit Spec-
      Metadaten
- [x] Ansichts-Teardown bei Navigation entfernt: Modelle werden auf
      dem existierenden ColumnView/GridView getauscht statt Widgets
      neu zu bauen — keine „broken accounting of active state" mehr,
      wenn ein Doppelklick die Navigation auslöst
- [x] Neues Anwendungs-Icon: geflügelter Walküren-Helm (eigene
      Identität)
- [x] Sparse-Attribute-Backends: alle FileInfo-Getter gehen durch
      has_attribute-Guards oder rohe attribute_*-Zugriffe — keine
      GLib-Criticals auf network:/// und ähnlichen GVFS-Orten
- [x] „In neuem Tab öffnen" + „In neuem Fenster öffnen"-Kontextmenü-
      Einträge auf Verzeichnissen; Mittelklick auf einen Ordner
      öffnet einen neuen Tab; neue Fenster sind unabhängige
      Valkyrie-Fenster, gewurzelt im gewählten Verzeichnis
- [x] Umbenennen via F2, Zeilen-Kontextmenü und Bearbeiten-Menü —
      nutzt den geteilten Namensdialog, Fehler melden sich als Dialog
- [x] Kopieren/Verschieben/Löschen-Fortschrittsfenster auf einem
      Worker-Thread mit Abbruch; rekursives Verzeichnis-Kopieren und
      Post-Order-Löschen (gio::File::copy ist shallow — tree_ops.rs
      läuft den Baum); globaler Byte-Fortschritt; Same-Dir-Moves und
      Drops in den eigenen Unterbaum einer Quelle gefiltert
- [x] Schnelle Copy-Engine (fast_copy.rs): FICLONE-Reflink zuerst
      versucht (instant Shared-Extents-Klon auf btrfs/xfs),
      gestreamte 4-MiB-Read/Write-Pipeline als universeller
      Fallback; Metadaten werden auf beiden Pfaden via GIO
      copy_attributes(ALL_METADATA) neu angewandt
- [x] Optionale Kopier-Integritätsprüfung: „Integrität prüfen"-
      Checkbox im Fortschrittsdialog (persistiert via GSettings
      verify-copies, mitten im Vorgang pro Datei schaltbar). Die
      Quelle wird inline gehasht, während ihre Bytes durch die
      Kopierschleife fließen — kein extra Quell-Lesen; das Ziel wird
      nachgehasht, solange es noch heiß im Page-Cache ist. Reflink-
      Kopien überspringen die Prüfung (gleiche Extents per
      Konstruktion); Abweichungen werden gesammelt und in einem
      Dialog gemeldet, ein sauberer Lauf bekommt eine Bestätigung
- [x] Fortschrittsdialog modernisiert: Operations-Icon + Titel,
      ellipsierte Aktueller-Eintrag-Zeile, Prozent-beschrifteter
      Balken, Detailzeile mit Bytes fertig/gesamt + geglätteter Rate
      + ETA, Live-„n/m geprüft"-Zähler während der Verifikation
- [x] Parallele Kopier-Option (copy_pool.rs + Einstellungsseite
      „Dateioperationen"): copy-parallel/copy-workers GSettings-
      Keys; der Tree-Walk, Verzeichnis-Erzeugung und Konflikt-
      Auflösung bleiben sequenziell, während Datei-Payloads von N
      Workern aus einer geteilten FIFO gezogen werden (UI deckelt N
      auf die CPU-Kernzahl, Default 4). Pool-Worker laufen denselben
      Reflink/Stream + Inline-BLAKE3-Pfad und buchen Bytes in den
      geteilten atomaren Zähler; Prüfergebnisse strömen pro Datei
      zurück. Hinweistext warnt, dass HDDs/Netzwerk-Mounts mit
      Parallelität langsamer werden können
- [x] Behoben: Kontextmenü Ausschneiden/Kopieren/Pfad-kopieren/
      Papierkorb/Upload wirkte nur auf die angeklickte Zeile, selbst
      innerhalb einer Mehrfachauswahl — Einfügen kopierte also nur
      eine Datei. FileView::action_targets() löst jetzt die
      effektive Zielmenge auf (volle Auswahl, wenn die gewählte Zeile
      Teil davon ist, sonst die Zeile allein) — dieselbe Regel wie
      andere Dateimanager
- [x] Fortschrittsdialog zeigt echte Parallelität: Pool-Worker melden
      die In-Flight-Anzahl, das Aktueller-Eintrag-Label liest sich
      „datei.iso (+1)", während mehrere Payloads gleichzeitig kopieren,
      statt zwischen Namen zu flackern
- [x] Papierkorb-Zeilenmenü „Endgültig löschen" honoriert jetzt die
      Auswahl: delete_forever nimmt die ganze action_targets()-Liste
      statt nur die angeklickte Datei
- [x] Raster-Zeilenabstände sind jetzt über Ordner hinweg uniform:
      GtkGridView meldet GTK_SIZE_REQUEST_CONSTANT_SIZE, also setzt
      gtk_widget_measure for_size im Request-Cache auf -1 zurück —
      die natürliche Höhe wird immer einspaltig berechnet (n_items
      Zeilen), der Viewport allokiert diese aufgeblähte Höhe und die
      Raster-Allokation klemmt jede Zeile auf allocated_height / 30 =
      Riesenlücken. Das neue hfw_bin.rs umhüllt das Raster, meldet
      HEIGHT_FOR_WIDTH und berechnet die echte Inhaltshöhe selbst
      (Modellanzahl + gemessene Größe einer realisierten Zelle +
      GTKs eigene Spaltenformel für die reale Breite). Visuell unter
      Xvfb verifiziert: 334 Ordner packen jetzt dicht und scrollen
      korrekt. Zusätzlich reservieren Captions exakt zwei Textzeilen
      (via Pango-Metriken gemessen) mit yalign 0 — jede Zelle
      bekommt identische, stabile Höhe; der Auto-Viewport nutzt
      hscroll-policy=minimum, sodass das Raster nie eine breitere-
      als-Fenster-Allokation bekommt
- [x] Automount: automount.rs mountet jedes mountbare, noch nicht
      gemountete Volume einmal beim Start und bei jedem
      volume-added (Hot-Plug) — das Boot-Medium der Live-Session
      erscheint jetzt gemountet statt als ausgegrauter Sidebar-
      Eintrag. Mount-Fehler bleiben bewusst still (die Sidebar-Zeile
      bleibt für manuellen Retry)
- [x] Papierkorb-Aktionen: Wiederherstellen via trash::orig-path,
      endgültig löschen, Papierkorb leeren — Bestätigungsdialoge für
      destruktive Schritte
- [x] Rekursive Suche (Strg+F + Toolbar-Button): Worker-Thread-
      Traversierung, Ergebnisse strömen durch die existierende
      Ansichts-Pipeline, gedebouncte Tastendrücke brechen vorherige
      Läufe ab (Generations-Zähler — kein SourceId::remove auf
      gefeuerte Sources). Traversierung bleibt auf dem Start-
      Dateisystem (unix::device, find -xdev-Semantik), sodass /proc,
      /sys, /run und Mounts übersprungen werden; verlinkte
      Verzeichnisse werden nie betreten (NOFOLLOW), was merged-usr-
      Doppeltreffer killt (/bin vs /usr/bin); Treffer, deren
      Attribute nicht mehr abgefragt werden können, werden
      verworfen statt als generische Dateien zu rendern
- [x] Drag & Drop: gdk::FileList-Payload auf Zeilen, Drop-Ziele auf
      Ordnerzeilen, Ansichts-Hintergrund und Tab-Titeln; Drop =
      verschieben, Strg+Drop = kopieren
- [x] Unit-Tests: unique_destination-Kollisionen, Quell-Filterung,
      rekursives Kopieren/Löschen, Such-Walk + Abbruch,
      Papierkorb-Ort-Erkennung — 15 Tests, alle grün
- [x] Namens-Konfliktdialog für Kopieren/Verschieben (conflict.rs):
      existierender vs. eingehender Eintrag Seite an Seite (Icon,
      Größe, geändert, erstellt) mit Ersetzen/Zusammenführen,
      Umbenennen (freie Namenseingabe, validiert, Slash lehnt ab),
      Überspringen, Abbrechen und „für alle übernehmen" (nie für
      Umbenennen gemerkt — jede Datei braucht ihren eigenen Namen);
      Worker parkt auf einem Antwort-Channel, solange der modale
      Dialog offen ist. tree_ops bekam move_tree mit Merge-Semantik,
      Umbenennen rekursiert Re-Auflösung, Kind-Mismatch-Guard für
      gemerkte Überschreiben-Antworten; Same-Dir-Einfügen
      (src == Ziel) auto-dupliziert als „name (2)" — dieser Fall ist
      eine Duplizier-Anfrage, kein Konflikt.
- [x] Gummiband-Mehrfachauswahl: enable_rubberband auf GridView und
      ColumnView; Mehrfachauswahl funktioniert mit Strg/Umschalt und
      Kopieren/Ausschneiden/Löschen wirken auf die ganze Auswahl
- [x] Seitentasten-Navigation überarbeitet: Tasten 8/9 werden von
      einem EventControllerLegacy in der Capture-Phase konsumiert
      (GestureClick auf 8/9 leakt in „any-button"-Gesten und
      korruptiert GTKs Sequenz-Buchhaltung), Navigation idle-
      verzögert
- [x] GTK-Bug umschifft: impliziter Pointer-Focus-Grab wird bei
      jedem Tastendruck erzeugt, aber nur für Tasten 1–5 gelöst
      (GdkModifierType hat keine Bits für 6+) — ein Seitentasten-
      Druck leakt einen Grab, der den nächsten Klick zum stale
      Target misroutet. Behoben durch kurzes Mappen eines
      unsichtbaren Helper-Popovers, dessen Grab stale implizite
      Grabs widerruft (gtkwindowgroup-Revoke-Pfad)
- [x] 18 Tests grün (Konflikt-Umbenennen/-Überspringen/-Merge-
      Abdeckung ergänzt)
- [x] Geräte-Mount/Unmount/Eject: gemountete Laufwerke bekommen einen
      Auswurf-Button und ein Rechtsklick-Menü (Öffnen/Aushängen/
      Auswerfen), ungemountete bieten Einhängen; mount_ops.rs führt
      die Async-Ops durch MountOperation und meldet Fehler via
      AlertDialog. Sidebar rebuilt explizit nach jeder Op (Daemon-
      Mounts emittieren mount-removed nicht zuverlässig). Sidebar-
      Split: Geräte leben in sidebar/devices.rs.
- [x] Klickbare Spaltenköpfe: jede Spalte trägt einen Feld-Sorter,
      das Modell kettet Verzeichnisse-zuerst + den ColumnViewSorter
      via MultiSorter, sodass Header-Klicks und Menü-Actions einen
      Zustand teilen (primary_sort_column/order); Sortierung
      persistiert bei jeder Änderung zu GSettings und seedet einmal
      beim Start
- [x] Einstellungsdialog (Bearbeiten → Einstellungen, Strg+,):
      Standard-Ansicht, versteckte Dateien, Standard-Icon-Größe,
      Terminal-Befehl-Override — live auf alle offenen Tabs
      angewandt, wo es sinnvoll ist
- [x] Einstellungen zu Sektionen erweitert (Ansicht / Sortierung /
      Terminal); die Terminal-Zeile ist jetzt eine Auswahl, die
      jeden auf PATH erkannten Emulator listet plus „Automatische
      Erkennung"
- [x] Statusleiste meldet Auswahl: „N Einträge — M Einträge
      ausgewählt (Größe)" via trn()-Plurale + glib::format_size;
      selection_summary in file_view/selection.rs zählt Objekte,
      überspringt Verzeichnis-Rekursion
- [x] System-Zwischenablage-Interop: set_clipboard publiziert
      x-special/gnome-copied-files (mit Ausschneiden/Kopieren-
      Leader) + text/uri-list via Union-ContentProvider; paste()
      liest die System-Zwischenablage asynchron, sodass in anderen
      Apps/Desktop-Shells kopierte Listen nach Valkyrie einfügen
      (cross-process unter Xvfb verifiziert)
- [x] Icon-Ansicht-Auswahlform: App-Stylesheet (data/valkyrie.css,
      beim Start eingebettet) macht das gestreckte Raster-Zellen-
      Highlight transparent und malt eine gerundete Accent-Box auf
      die Item-Box (.file-cell.selected, getoggelt aus
      ListItem::selected); Theme-12px-Zellenmargin auf 4px reduziert.
      GTK-Knoten ist „child" (gegen das libgtk-Theme verifiziert),
      @accent-Variablen auf barem GTK4 nicht verfügbar — Auswahl-
      Accent hartkodiert auf den Theme-Wert. Auswahl zwischen den
      beiden MultiSelection-Modellen beim Ansichtswechsel gespiegelt
- [x] Icon-Karten & Totzonen-Klicks: Item-Box füllt jetzt ihre
      Rasterzelle (uniforme Karte, hexpand+fill), sodass ein Klick
      neben einem Icon nicht mehr auf einem Nachbarn landen kann; die
      ~4px Inter-Zell-Lücke plus alle Hintergrund-Flächen löschen
      die Auswahl via Button-1-Geste, die die Sequenz verweigert
      (Gummiband unberührt). Lange Namen brechen Wort-Zeichen auf
      max. 2 Zeilen + Ellipse (nur Anzeige, echter Name bleibt,
      Tooltip zeigt ihn)
- [x] Zoom-Persistenz behoben: GVFS-Metadaten akzeptieren nur
      Strings — attribute_int32 schrieb nichts; jetzt als
      Dezimal-String gespeichert und zurück geparst (über App-
      Neustart verifiziert)
- [x] Valkyrie-Identität: icon/icon.png installiert als
      hicolor/512x512/apps/valkyrie.png (SVG-Fallback behalten);
      gebündeltes Theme/Valkyrie beim Start via gtk-icon-theme-name
      aktiviert, wenn unter einem XDG-Icon-Verzeichnis installiert
      (per-Nutzer oder System), erbt Adwaita/hicolor. Altes
      Platzhalter-SVG entfernt — icon.png ist die einzige Quelle
      (hicolor/512x512 + Deb-Asset)
- [x] Icon-Ansicht-Lücken sind echt: GTK löst Klicks über die
      nächste Kachel auf, also ist nur border-spacing zwischen
      Kacheln echte Totzone — Raster-border-spacing 8px hält die
      Streifen zwischen Karten als klickbaren Hintergrund (Auswahl
      löscht, kein Nachbar aktiviert) ohne aufgebläht zu wirken
- [x] Übergroße Item-Abstände behoben: das Stock-Theme gibt jeder
      Raster-Kind-Box ein unsichtbares 12px-Margin + 8px-Border-
      Spacing und der Zelle selbst 3px-Padding — das polsterte jede
      Karte ~30px breiter, kollabierte die Spaltenzahl und verteilte
      Einträge weit auseinander (am schlimmsten in Ordnern mit
      gespeichertem höherem Zoom wie Downloads, weil GtkGridView die
      Fensterbreite gleichmäßig durch die Spaltenzahl teilt:
      Karte 124px → 5 Spalten → ~40px Tot-Slack pro Kachel). Das CSS
      resettet jetzt Zellen-Margin/Padding und Karten-Margin/
      Border-Spacing, und die Karten-Anfrage ist px+14 — nur das
      Icon plus sein inneres Padding. Downloads (Zoom 104) packt
      jetzt 6 Spalten bei ~136px Pitch, passend zur Dichte des Home-
      Verzeichnisses
- [x] Scroll-Bereich behoben: GtkGridView fehlt get_request_mode,
      also behandelt GTK es als CONSTANT_SIZE und misst seine Höhe
      bei der min_columns-Belegung für jede Anfrage — das Scrolled-
      Fenster allokierte eine enorm übergroße Inhaltshöhe.
      scroll_clamp berechnet die echte Höhe aus realisierten Zellen
      neu (Spaltenzahl aus First-Row-Zellen, Pitch aus Zeilen-Delta)
      und klemmt das vadjustment-Upper zurück — unter Xvfb
      verifiziert
- [x] Optionale Berechtigungen beim Anlegen: Neuer-Ordner-/Neues-
      Dokument-Dialoge bekamen einen eingeklappten rwx-Expander
      (Eigentümer/Gruppe/Andere-Matrix, live-Oktal im Label),
      voreingestellt auf 0755/0644 — eingeklappt behält die umask-
      Defaults, ausgeklappt wendet die Bits via unix::mode nach dem
      Anlegen an. Ende-zu-Ende verifiziert (0744 angewandt),
      unit-getestet
- [x] Zoom-Anzeige: Tooltip am Regler trägt die aktuelle Icon-Größe
      („Zoom: N", live aktualisiert) — eine permanente Anzeige hatte
      die Statusleiste zu hoch gemacht; Bereich 16–300, Schema +
      Einstellungen aktualisiert
- [x] Icon-Aspekt behoben: die fixe Höhenanfrage der Karte quetschte
      Bild + Beschriftung in zu wenig Raum und plattete das Icon —
      Höhe ist jetzt natürlich (uniforme Kartenbreite behalten),
      Ordner sehen bei jedem Zoom richtig aus
- [x] Standard-Icon-Größe von 48 → 80 erhöht (GSettings-Schema +
      Fallback; Per-Verzeichnis-GVFS-Zoom gewinnt weiterhin, wo
      bereits gespeichert)
- [x] Nextcloud-Integration: Server-URL + Benutzername in den
      Einstellungen (GSettings nextcloud-server/-user; http(s)://-
      Eingabe normalisiert), App-Passwort verschlüsselt im System-
      Keyring via secret-tool/libsecret mit sichtbarem Status +
      „Passwort vergessen" (secrets.rs spawnnt secret-tool, Passwort
      auf stdin — nie in argv, GSettings oder Code). Sidebar-
      „Nextcloud"-Zeile unter Netzwerk mountet
      davs://<host>/remote.php/dav/files/<user>/ mit einer aus dem
      Keyring gefütterten MountOperation und browst darin;
      Einstellungsänderungen aktualisieren die Zeile live.
      Kontextmenü „Zu Nextcloud hochladen…" öffnet eine Zielordner-
      Auswahl (nextcloud/dialog.rs — enumeriert die Remote-Root auf
      einem Worker-Thread durch einen Channel + Main-Context-Poll,
      optionaler neuer Unterordner wird vor dem Kopieren angelegt)
      und leitet durch progress::copy_move, sodass Fortschritt,
      Abbruch und der Namens-Konfliktdialog unverändert greifen.
      Zeile bleibt verborgen, bis Server + Nutzer konfiguriert sind.
      35 Tests grün.
- [x] Synchronisierte Ordner (permanente Lokal↔Nextcloud-Paare):
      jedes Paar ist ein lokales Verzeichnis + Remote-Pfad +
      Richtung (nur-hochladen-Spiegelung oder Zwei-Wege) + Zeitplan
      (manuell / 5 / 15 / 60 Min / live bei Änderung). Gespeichert
      in GSettings nextcloud-sync-folders (a(sssi)); verwaltet in
      Einstellungen → Nextcloud (folders_ui.rs: Liste + Hinzufügen-
      Dialog mit Ordnerauswahl, Richtungs- und Zeitplan-Dropdowns;
      Entfernen löscht auch das Journal). Kontextmenü „Mit Nextcloud
      synchronisieren…" auf einem lokalen Verzeichnis öffnet den
      Paar-Dialog vorbefüllt oder führt „Jetzt synchronisieren" auf
      einem existierenden Paar aus. Engine: reines plan.rs (12 Unit-
      Tests) bildet lokale/Remote-Listings + das Journal auf eine
      Op-Liste ab (Upload/Download/DeleteLocal/DeleteRemote/
      ConflictUpload/ConflictDownload); exec.rs führt via tree_ops
      aus; sync.rs mountet, listet, plant, führt aus und journaliert;
      monitor.rs treibt Intervall-Timer und gedebouncte
      GFileMonitor-Live-Watches, bei jeder Einstellungsänderung
      abgerissen und neu gebaut.
      Journal (state.rs): Per-Seite-(mtime,size)-Fingerprints, nach
      den Ops aus BEIDEN Bäumen neu gelesen — WebDAV bewahrt Upload-
      mtimes nicht, also würden journalierte Erwartungen für immer
      re-konfliktieren. Beidseitige Bearbeitungen behalten die
      neuere Datei unter dem Namen, der Verlierer wird
      „<name> (Konflikt)"; Löschungen propagieren nur für Pfade,
      die auf beiden Seiten journaliert sind. 52 Tests grün.
- [x] Sidebar-Orte-Kontextmenü: Rechtsklick auf Home/Desktop/Recent/
      Dateisystem/Papierkorb bietet In neuem Tab öffnen / In neuem
      Fenster öffnen, plus Im Terminal öffnen, wo das Ziel ein echter
      Pfad ist; Ende-zu-Ende unter Xvfb verifiziert (neuer Tab auf
      Home geöffnet).
- [x] GTK-measure-Warnung behoben: der Ansichts-Stack hatte
      hhomogeneous/vhomogeneous=true (Default) → GTK maß auch das
      unsichtbare ColumnView (Mindestbreite 640) transient mit
      Breite 0 → „Trying to measure ... needs at least 640". Stack
      jetzt nicht-homogen: nur das sichtbare Kind wird gemessen.
      (Lazy-Listenansicht als Alternative verworfen — sie hätte die
      geteilte Sortier-Pipeline ohne ColumnView-Sorter still gelegt.)
- [x] Öffnen mit: Datei-Kontextmenüs tragen ein „Öffnen mit"-
      Untermenü, gespeist von gio::AppInfo::recommended_for_type
      (App-Icons + Namen, nur Dateien, nie im Papierkorb). „Andere
      Anwendung…" öffnet eine filterbare Auswahl aller installierten
      Apps mit optionaler „Immer für diesen Dateityp verwenden"-
      Checkbox (set_as_default_for_type +
      set_as_last_used_for_type). Unter Xvfb verifiziert: PNG-
      Untermenü listete 6 registrierte Apps, Klick auf GIMP startete
      es mit der Datei, der Auswahldialog öffnet.
- [x] Einstellungen als Kategorie-Navigation neu gestaltet:
      GtkStackSidebar links (Ansicht / Sortierung / Terminal /
      Nextcloud), die aktive Seite rechts, jede Seite in eigenem
      ScrolledWindow — der Dialog bleibt kompakt, egal wie viele
      Sektionen wachsen. Unter Xvfb verifiziert (Sidebar-Wechsel
      funktioniert, Seiten rendern).
- [x] „Broken accounting of active state"-Warnungen behoben: die
      Zeilen-, Hintergrund- und Orte-Menüs öffneten ihre Popovers
      bei Tastendruck; wenn der Pointer sich vor dem Loslassen
      bewegte, brach GTKs Active-State-Buchhaltung entlang der
      gedrückten Widget-Ahnenreihe (GTK-Bug #4563, upstream behoben
      in GTK 4.21.6 — dieses System läuft 4.18.6). Alle Rechtsklick-
      Gesten popen jetzt beim LOSLASSEN, die X11-Menü-Konvention
      (devices.rs machte das bereits); Orte beanspruchen die Sequenz
      weiterhin beim Druck, sodass andere Zeilen-Handler unterdrückt
      bleiben. Unter Xvfb verifiziert: alle drei Menüs öffnen
      zuverlässig nach Druck-Bewegen-Loslassen, null Warnungen.
- [x] Rendering-/Scrolling-Performance-Pass: Listen-Factories folgen
      jetzt dem GTK4-Recycling-Vertrag — Widget-Konstruktion in
      connect_setup, reine Daten-Updates in connect_bind (vorher
      wurde bei jedem Bind ein voller Widget-Baum neu gebaut).
      Zeilen-Gesten und DnD lösen item.item() zur Event-Zeit auf,
      sodass recycelte Zeilen auf das aktuell gebundene Objekt
      wirken. Selection-CSS-Handler wird einmal in setup verbunden
      statt einmal pro Bind (behebt ein Per-Scroll-Schritt-Handler-
      Leck). Raster-Factory nach src/view_grid.rs umgezogen.
- [x] Thumbnail-Pipeline neu geschrieben: begrenzter 4-Worker-Pool
      auf einer geteilten mpsc-Queue statt einem OS-Thread + einem
      50-ms-Timer pro gebundenem Item; Ergebnisse kehren via
      thread-sicherem glib::idle_add_once zurück; ein Per-Bild-
      Generations-Zähler verhindert Stale-Writes auf recycelte
      Zellen; dekodierte Texturen in einem 512-Eintrag-FIFO gecacht,
      sodass Zurückscrollen instant ist.
- [x] Sortier-Komparator zero-alloc: Lowercase-Sortierschlüssel +
      Dotfile-Flag pro FileObject bei Konstruktion gecacht (war: zwei
      frische Lowercase-Strings pro Vergleich — ~130k Allocs für 10k
      Dateien).
- [x] „Andere Anwendung…"-Auswahl startete die falsche App: sie
      schlug die gewählte Zeile via ListBoxRow::index() nach, was die
      *sichtbare* Position ist — nach Sortieren/Filtern stimmte das
      nicht mehr mit der AppInfo-Listenreihenfolge überein, sodass
      eine unbeteiligte App startete. Zeilen tragen jetzt eine
      explizite (Zeile, App)-Assoziation; Ende-zu-Ende unter Xvfb
      mit einer Marker-Desktop-Datei verifiziert (gefilterter Index
      0, korrekte App gestartet).
- [x] Cloud-Unterstützung von Nextcloud-only zu jedem WebDAV-
      Anbieter verallgemeinert: das Server-Feld akzeptiert einen
      nackten Host (Nextcloud/ownCloud-Konvention
      /remote.php/dav/files/<user> wird angenommen) oder eine volle
      URL, deren Pfad wörtlich als DAV-Endpunkt genutzt wird
      ({user}-Platzhalter expandiert zum Login; Pfad + Userinfo sind
      URI-escaped, sodass E-Mail-artige Logins funktionieren).
      Nutzer-sichtbare Strings sagen jetzt „Cloud"/„WebDAV"
      (Einstellungsseite, Kontextmenü, Upload- und Paar-Dialoge,
      Keyring-Label); die Sidebar-Zeile zeigt den konfigurierten
      Host. Interne GSettings-Keys behalten das nextcloud-*-Präfix,
      sodass existierende Konfigurationen überleben. 4 neue Parse-
      Tests (57 gesamt).
- [x] Oktal-Berechtigungseingabe: ein geteiltes `mode_editor`-Modul
      rendert die rwx-Matrix plus ein editierbares Oktal-Feld, in
      beide Richtungen synchronisiert — Tippen von „755"/„777"
      aktualisiert die Checkboxen, Toggles aktualisieren das Feld;
      Commits geschehen bei Enter/Fokus-Verlust, ungültige oder
      Special-Bit-Eingabe (1777) wird mit Fehler-Styling abgelehnt
      statt still maskiert. Genutzt von der Eigenschaften-Seite
      „Berechtigungen" (sofortiges chmod; Bits außerhalb der neun
      rwx-Positionen — Typ, setuid/setgid/sticky — bleiben beim
      Schreiben erhalten, Fehlschlag revertiert Widgets) und vom
      Neuer-Ordner-/Neues-Dokument-Expander. 3 neue Parse-/Format-
      Tests (61 gesamt).
- [x] Oktal-Eingabe wendet live an: ein kompletter 3-stelliger
      Oktal-Wert committet beim Tastendruck, der ihn vervollständigt
      (Checkboxen + chmod folgen sofort); Fragmente bleiben pending
      bis Enter/Fokus-Verlust, sodass Zwischenwerte wie „7" keine
      Berechtigungen umkippen können, und ungültige Eingabe markiert
      das Feld, ohne den Text mitten beim Tippen zu löschen.
- [x] Eingebettetes Terminal-Panel (): Ansicht → „Terminal-Panel"
      oder F4 dockt eine echte VTE-Shell am Fensterboden an —
      getrennt vom Kontextmenü-„Im Terminal öffnen", das weiterhin
      ein eigenständiges Fenster öffnet. Die Shell spawnt lazy im
      Ordner des aktiven Tabs und folgt jeder Navigation/jedem
      Tab-Wechsel via `cd` (POSIX-Single-Quote-Escaping); Remote-
      GVFS-Orte ohne lokalen Pfad werden übersprungen. Sichtbarkeit
      + Panel-Höhe persistieren via GSettings (terminal-panel-
      visible, terminal-panel-height); der Schließen-Button im
      Header toggelt dieselbe win.terminal-panel-Action. 4 neue
      shell_quote-Tests (76 gesamt); unter Xvfb verifiziert (Spawn-
      cwd, Tastatureingabe, cd-Sync bei Navigation).
- [x] Geteilte Ansicht (, opt-in): Ansicht → „Geteilte Ansicht" oder
      F3 teilt den aktiven Tab in zwei unabhängige Panes (PaneView
      mit eigener Zurück/Vorwärts/Hoch-Historie + Breadcrumb). Die
      zuletzt benutzte Pane besitzt Toolbar/Suche/Zoom/Status;
      Zeilen-Kontextmenüs bekommen „In andere Ansicht kopieren/
      verschieben", aufgelöst durch eine Per-View-Sibling-Registry.
      BrowserTabs Split-State lebt in Rc-geteilten Zellen, sodass
      von Action-Handlern gemachte Klone dieselbe Pane beobachten
      (behoben: Aktivieren in der Sekundär-Pane dead-referenzierte
      das gedroppte Rc). Unter Xvfb verifiziert: F3-Split rendert
      beide Panes, Doppelklick navigiert nur die fokussierte Pane.
- [x] Verzeichnis-Überwachung überlebt Löschen+Neuanlegen:
      GtkDirectoryLists interner Monitor ist an die Inode gebunden,
      auf der er startete — Löschen und Neuanlegen des angezeigten
      Verzeichnisses (lb build/clean auf chroot/, binary/, cache/;
      git-Checkouts; atomare Verzeichnis-Swaps) tötete den Watch
      still und fror die Ansicht beim letzten Inhalt ein bis F5.
      Neues dir_watch-Modul überwacht das *Parent*-Verzeichnis und
      lädt die Ansicht neu, wenn der Ordner wieder erscheint
      (Created/MovedIn); der Destroy-Handler des Widgets bricht den
      Monitor→Callback→View-Referenzzyklus. 2 neue Tests
      (78 gesamt); unter Xvfb verifiziert — in einem neu angelegten
      Verzeichnis erstellte Dateien erscheinen jetzt automatisch.
- [x] Sidebar-Mount-Race mit dem Automounter: Klick auf eine
      Gerätezeile, während der Automounter noch mountete (erstes
      Fenster nach Start), scheiterte mit „bereits eingehängt" /
      einem Mount-Fehlerdialog, und ein stale GVolume-Objekt konnte
      weiterhin keinen Mount melden. mount_and_open löst jetzt
      zuerst den Live-Mount auf (via unix-device-Identifikator des
      Volumes gegen die Mount-Liste des Monitors, nicht nur
      get_mount()), navigiert direkt hinein und wartet bei einem
      fehlgeschlagenen Mount-Request bis zu 3s auf den In-Flight-
      Mount des Automounters, bevor ein Fehler gezeigt wird.
- `fxlabel.rs`/`fxcfg.rs`: weitere Effekt-Modi
  (Regenbogen-Zyklus, Typewriter) denkbar.
