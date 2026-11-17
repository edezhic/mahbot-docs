#!/bin/sh
# Fetch the PDFium release the comparison records, once, into a development location outside the
# repository. It never runs as part of a build or a test; a machine without the library refuses the
# comparison rather than passing it.
#
# Usage: ./fetch-pdfium.sh [release-directory]
#
# The release directory defaults to the one rasteriser.json names; the archive is downloaded, its
# digest checked against the record, and only then extracted. The library's own digest is checked
# too. An existing library with the recorded digest is left alone.

set -eu

here=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
record="$here/rasteriser.json"

# The record is small JSON with one string per line; no interpreter is needed to read it.
field() {
    sed -n "s/.*\"$1\"[[:space:]]*:[[:space:]]*\"\([^\"]*\)\".*/\1/p" "$record" | head -n 1
}

digest() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$1" | cut -d' ' -f1
    else
        shasum -a 256 "$1" | cut -d' ' -f1
    fi
}

release=$(field release)
platform=$(field platform)
library=$(field library)
expected=$(field sha256)
expected_archive=$(field archive_sha256)
cache="${1:-$HOME/.cache/page-oracle/pdfium/$(printf '%s' "$release" | tr / -)}"

target="$cache/lib/$library"
if [ -f "$target" ]; then
    actual=$(digest "$target")
    if [ "$actual" = "$expected" ]; then
        printf '%s\n' "$target"
        exit 0
    fi
    printf 'refusing: %s is %s, and the record states %s\n' "$target" "$actual" "$expected" >&2
    exit 1
fi

mkdir -p "$cache"
archive="$cache/pdfium-$platform.tgz"
url="https://github.com/bblanchon/pdfium-binaries/releases/download/$release/pdfium-$platform.tgz"
if [ -f "$archive" ] && [ "$(digest "$archive")" != "$expected_archive" ]; then
    rm -f "$archive"
fi
if [ ! -f "$archive" ]; then
    printf 'fetching %s\n' "$url" >&2
    curl -fsSL -o "$archive" "$url"
fi
actual_archive=$(digest "$archive")
if [ "$actual_archive" != "$expected_archive" ]; then
    printf 'refusing: %s is %s, and the record states %s\n' "$archive" "$actual_archive" "$expected_archive" >&2
    exit 1
fi
tar xzf "$archive" -C "$cache"
actual=$(digest "$target")
if [ "$actual" != "$expected" ]; then
    printf 'refusing: %s is %s, and the record states %s\n' "$target" "$actual" "$expected" >&2
    exit 1
fi
printf '%s\n' "$target"
