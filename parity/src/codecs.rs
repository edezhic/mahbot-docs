// The codecs whose resolved versions are part of the comparison identity: the JPEG encoder and
// decoder, the PDF parser, rasterizer and image decoders a page raster reaches through, the legacy
// and OOXML parsers, the ZIP/deflate layer, and the PDF object model the crate's page actions copy
// pages with. The reader arms and the writer's canonical model decode with these, so a bump can
// change a compared digest without changing any source this workspace holds; the object model is
// recorded for the same reason, because a new version of it can write a different file.
//
// This file is the list's one home: `build.rs` includes it to emit the versions, and
// `tests/engine_contract.rs` reads it through the crate. Adding a codec here is therefore covered
// by the contract test without a second copy to keep in step.

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
    "vello_cpu",
    "vello_common",
    "peniko",
    "kurbo",
    "pic-scale",
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

/// `name=version[,version…];…` for [`CODECS`], read from the workspace lockfile's text. Every
/// resolved copy is named, because the lock can hold more than one version of a crate and a change
/// to any of them is a change to what may run. A codec missing from the lock is a build-script bug,
/// not a reason to record an incomplete identity.
pub fn codec_versions(lock: &str) -> String {
    let mut versions: std::collections::BTreeMap<&str, Vec<&str>> =
        std::collections::BTreeMap::new();
    let mut name: Option<&str> = None;
    for line in lock.lines().map(str::trim) {
        if let Some(rest) = line.strip_prefix("name = \"") {
            name = rest.strip_suffix('"');
        } else if let Some(rest) = line.strip_prefix("version = \"")
            && let Some(version) = rest.strip_suffix('"')
            && let Some(name) = name.take()
            && CODECS.contains(&name)
        {
            versions.entry(name).or_default().push(version);
        }
    }
    CODECS
        .iter()
        .map(|name| {
            let mut resolved = versions
                .get(name)
                .unwrap_or_else(|| panic!("Cargo.lock resolves no `{name}`"))
                .clone();
            resolved.sort_unstable();
            format!("{name}={};", resolved.join(","))
        })
        .collect()
}
