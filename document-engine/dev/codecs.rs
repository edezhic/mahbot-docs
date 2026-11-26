// The codecs whose resolved versions are part of the comparison identity: the JPEG encoder and
// decoder, the PDF parser, rasterizer and image decoders a page raster reaches through, the legacy
// and OOXML parsers, the ZIP/deflate layer, and the PDF object model the crate's page actions copy
// pages with. The reader arms and the writer's canonical model decode with these, so a bump can
// change a compared digest without changing any source this workspace holds; the object model is
// recorded for the same reason, because a new version of it can write a different file.
//
// This file is the list's one home: the engine identity digests it, and `tests/engine_contract.rs`
// holds it to the lockfile. Adding a codec here is therefore covered by the contract test without a
// second copy to keep in step.
//
// Every resolved copy is named, so a crate the gate alone compiles joins the engine's provenance:
// the `hayro 0.7.1` the product's reader sources and the canonical PDF model parse with sits beside
// the crate's own `0.9.0`, and the engine identity therefore moves when either pin does. That is
// deliberate — the identity says which bytes were in the build, not which of them the library
// reaches.

/// The codecs whose resolved versions are part of the comparison identity. A codec named in a
/// manifest is pinned exactly by the contract test, so a patch bump is a deliberate edit.
pub const CODECS: &[&str] = &[
    "image",
    "zune-core",
    "zune-jpeg",
    "png",
    "hayro",
    "hayro-syntax",
    "hayro-interpret",
    "hayro-cmap",
    "hayro-jbig2",
    "hayro-ccitt",
    "hayro-jpeg2000",
    // The PostScript scanner a CMap program is tokenized with, and the Brotli decompressor the
    // embedded CMaps are unpacked with (`brotli` is its facade): what a character is mapped to, and
    // so which glyph a character names. A glyph's *outline* is the font group's business, above.
    "hayro-postscript",
    "brotli",
    "brotli-decompressor",
    // The font program parser and the rasterizer the renderer draws a glyph with: what a glyph's
    // outline is and which pixels it covers. `font-types` is the OpenType table-type and number
    // layer `read-fonts` parses through, `glifo` holds the glyph-outline processing `vello_cpu`
    // runs, and `fearless_simd` is the kernel dispatch every SIMD path below it chooses from, so a
    // bump in any of them can move a raster's bytes without moving a version this crate names.
    "hayro-font",
    "font-types",
    "ab_glyph_rasterizer",
    "glifo",
    "fearless_simd",
    "vello_cpu",
    "vello_common",
    "peniko",
    "kurbo",
    "pic-scale",
    // The filter math `pic-scale` resamples with: a bump there moves a downscaled page's pixels as
    // surely as the resampler that calls it.
    "pxfm",
    "moxcms",
    "skrifa",
    "read-fonts",
    "pdf-extract",
    "lopdf",
    "office_oxide",
    "zip",
    "quick-xml",
    "flate2",
    // The link parser a created Word document's addresses are written with, and the crates beneath
    // it that decide a compared target: `idna` applies the URL standard's IDNA mapping,
    // `idna_adapter` is what applies UTS-46's mapping, `icu_normalizer` and `icu_properties` hold
    // that mapping and its data, and `zerotrie` holds the trie it reads; `percent-encoding` and
    // `form_urlencoded` decide the percent-encoding of a target. A bump in any of them is a change
    // to a compared byte and belongs in the identity. The generic containers those crates are built
    // on — `smallvec`, `yoke`, `zerovec`, `tinystr` and the rest — carry no mapping or encoding of
    // their own, so a bump there cannot decide a compared byte and is not recorded.
    // `cargo tree -p url --offline` resolves the tree from the local cargo cache: url 2.5.8 →
    // idna 1.1.0 → idna_adapter 1.2.2 → icu_normalizer 2.2.0 / icu_properties 2.2.0, and
    // icu_provider 2.4.0 → zerotrie 0.2.5.
    "url",
    "idna",
    "idna_adapter",
    "percent-encoding",
    "form_urlencoded",
    "icu_normalizer",
    "icu_normalizer_data",
    "icu_properties",
    "icu_properties_data",
    "icu_provider",
    "icu_locale_core",
    "icu_collections",
    "zerotrie",
];

/// The workspace lockfile's text, or `""` where the crate is built without one.
///
/// The identity is read from the working tree when a development-only target runs, so the lock is
/// found by walking up from the crate root: an ordinary build never reads it, and a crate vendored
/// without a lockfile — or under a tree whose lockfile resolves none of [`CODECS`] — still has an
/// identity rather than a build failure.
pub fn lock() -> &'static str {
    static LOCK: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    LOCK.get_or_init(|| lock_beside(std::path::Path::new(env!("CARGO_MANIFEST_DIR"))))
}

/// The nearest lockfile at or above `start`, read; `""` where there is none.
fn lock_beside(start: &std::path::Path) -> String {
    let mut directory = Some(start);
    while let Some(current) = directory {
        let candidate = current.join("Cargo.lock");
        if candidate.is_file() {
            // A lockfile that exists but cannot be read is absent, not fatal: this is provenance,
            // and a consumer's build must not fail over it.
            return std::fs::read_to_string(&candidate).unwrap_or_default();
        }
        directory = current.parent();
    }
    String::new()
}

/// `name=version[,version…];…` for [`CODECS`], read from [`lock`]. Empty where there is no lockfile.
pub fn codec_versions() -> String {
    codec_versions_of(lock())
}

/// [`codec_versions`] over a lockfile's text. Every resolved copy is named, because the lock can
/// hold more than one version of a crate and a change to any of them is a change to what may run. A
/// codec the text does not resolve is left out rather than failing the run: the identity degrades to
/// what the file states, and the lock this crate ships is held to [`CODECS`] by
/// `tests/engine_contract.rs`, which is where a missing codec is a defect.
pub fn codec_versions_of(lock: &str) -> String {
    if lock.is_empty() {
        return String::new();
    }
    CODECS
        .iter()
        .filter_map(|name| {
            let resolved = resolved_versions(lock, name);
            (!resolved.is_empty()).then(|| format!("{name}={};", resolved.join(",")))
        })
        .collect()
}

/// The versions the lock resolves for `name`, sorted; empty when it holds no such package.
pub(crate) fn resolved_versions<'a>(lock: &'a str, name: &str) -> Vec<&'a str> {
    let mut versions = Vec::new();
    let mut current: Option<&str> = None;
    for line in lock.lines().map(str::trim) {
        if line == "[[package]]" {
            current = None;
        } else if let Some(rest) = line.strip_prefix("name = \"") {
            current = rest.strip_suffix('"');
        } else if let Some(rest) = line.strip_prefix("version = \"")
            && let Some(version) = rest.strip_suffix('"')
            && current == Some(name)
        {
            versions.push(version);
        }
    }
    versions.sort_unstable();
    versions
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The read-failure branch of [`lock_beside`], which no ordinary run reaches.
    #[cfg(unix)]
    #[test]
    fn a_lockfile_that_cannot_be_read_is_absent_rather_than_fatal() {
        use std::os::unix::fs::PermissionsExt as _;
        let area = tempfile::tempdir().expect("a temporary directory");
        let lock = area.path().join("Cargo.lock");
        std::fs::write(
            &lock,
            "[[package]]\nname = \"image\"\nversion = \"0.25.10\"\n",
        )
        .expect("a lockfile");
        std::fs::set_permissions(&lock, std::fs::Permissions::from_mode(0o000))
            .expect("no read permission");
        if std::fs::read_to_string(&lock).is_ok() {
            // Root reads it regardless; the branch is not reachable here.
            std::fs::set_permissions(&lock, std::fs::Permissions::from_mode(0o600))
                .expect("restore the permission");
            return;
        }
        assert_eq!(lock_beside(area.path()), "");
        std::fs::set_permissions(&lock, std::fs::Permissions::from_mode(0o600))
            .expect("restore the permission");
    }

    #[test]
    fn a_lockfile_that_resolves_no_codec_records_what_it_can() {
        // A crate vendored under a tree whose lockfile knows none of `CODECS` still has an identity
        // rather than a failed run; the shipped lock is held to the list by `engine_contract`.
        assert_eq!(codec_versions_of(""), "");
        assert_eq!(
            codec_versions_of("[[package]]\nname = \"something-else\"\nversion = \"1.0.0\"\n"),
            ""
        );
        assert_eq!(
            codec_versions_of("[[package]]\nname = \"image\"\nversion = \"0.25.10\"\n"),
            "image=0.25.10;"
        );
    }
}
