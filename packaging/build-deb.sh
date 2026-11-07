#!/usr/bin/env bash
# Build the valkyrie .deb package: release binary plus the desktop
# integration data (desktop entry, GSettings schema, icon, icon theme,
# gettext catalogs). cargo-deb stages everything declared in the
# [package.metadata.deb] assets table of Cargo.toml and resolves the
# shared-library dependencies itself ($auto -> dpkg-shlibdeps).
#
# Output: target/debian/valkyrie_<version>-<rev>_amd64.deb
set -euo pipefail
cd "$(dirname "$0")/.."

cargo build --release

# cargo-deb copies files verbatim — it does not compile .po sources —
# so the gettext catalogs are staged under target/locale/ first.
for lang in de en; do
    out="target/locale/$lang/LC_MESSAGES"
    mkdir -p "$out"
    msgfmt --check "po/$lang.po" -o "$out/valkyrie.mo"
done

# --no-build: the release binary above is fresh; cargo-deb would
# rebuild it otherwise.
cargo deb --no-build
