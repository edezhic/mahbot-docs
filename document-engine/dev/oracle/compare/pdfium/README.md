# PDFium, the outside rasteriser

Development only. Nothing here is linked into `document-engine` or the product.

| | |
|---|---|
| Program | PDFium |
| Release | `chromium/8165` |
| Version | 160.0.8165.0 |
| Platform | `mac-arm64` |
| Library | `libpdfium.dylib`, SHA-256 `d421e426bd487de02fa3bd1f2aeaf56947261f790e6116951e26f729ced1b018` |
| Archive | `pdfium-mac-arm64.tgz`, SHA-256 `112e74d717f3e0310273b70f3c8780e8a61ff11b34834124465c8305c841d350` |
| Source | https://github.com/bblanchon/pdfium-binaries/releases/download/chromium/8165/pdfium-mac-arm64.tgz |

`../rasteriser.json` states the release, the platform, the library digest and the archive digest the
comparison refuses against. The library itself is **not committed**: it is fetched once into a
development location outside the repository, by default

```
~/.cache/page-oracle/pdfium/chromium-8165/lib/libpdfium.dylib
```

Run `../fetch-pdfium.sh` (it downloads the archive, checks the archive digest, extracts, and checks
the library digest), or fetch the archive by hand and check both digests:

```
mkdir -p ~/.cache/page-oracle/pdfium/chromium-8165
curl -fsSL -o /tmp/pdfium-mac-arm64.tgz \
  https://github.com/bblanchon/pdfium-binaries/releases/download/chromium/8165/pdfium-mac-arm64.tgz
shasum -a 256 /tmp/pdfium-mac-arm64.tgz
tar xzf /tmp/pdfium-mac-arm64.tgz -C ~/.cache/page-oracle/pdfium/chromium-8165
shasum -a 256 ~/.cache/page-oracle/pdfium/chromium-8165/lib/libpdfium.dylib
```

`../fetch-pdfium.sh [directory]` fetches into another release directory. The comparison's
`--library <file>` names a library elsewhere. Either way the digest is checked; a run against another library
refuses rather than producing a diff that means nothing.

## Licences

`LICENSE-pdfium.txt` is PDFium's own BSD-3-Clause notice. It is the rasteriser's licence, and
`../rasteriser.json` names it in its `licence` field. `LICENSE` is the MIT notice of the
`pdfium-binaries` packaging the archive comes from; it is committed because the fetch script
downloads that archive. The packaging's notice points to its own `licenses/` directory for the
archive's other third-party components, and none of those is committed: this comparison drives
PDFium alone.

## Upgrading

An upgrade is a deliberate change, in this order: fetch another release, update
`../rasteriser.json` (release, version, platform, library, library digest `sha256`, archive digest
`archive_sha256`, and `licence` if the notice changes), run
`cargo test -p document-engine --test oracle -- --ignored --exact measure`, and write the
numbers it prints into
`../calibration.json` together with the new release and digest. `--measure` is the one mode that does
not require the calibration to agree with the record — it is the mode that writes one — so it runs
against the release the record now names; a comparison run refuses until the calibration names that
same release and digest. `../fetch-pdfium.sh` refuses an archive whose digest is not the one the
record states, and a run refuses a library whose digest differs; both digests therefore have to be
updated together. The thresholds are calibrated against the exact library digest the record names; a
different library is a different drawing.
