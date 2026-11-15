//! A created link's address: parsed and checked where it is written.
//!
//! # The address the parse produced
//!
//! The reference parses the address with the host's own URL parser and writes what the parse
//! produced, not the string it was given: parsing trims the space around it, lower-cases the scheme
//! and the host, drops a default port, appends the empty path's `/`, and percent-encodes or
//! punycodes what the standard says to. A hand-written subset would diverge on shapes no case
//! records, so the crate uses a real parser, pinned exactly, whose version joins the comparison's
//! recorded identity.

use crate::operation::Failure;
use crate::vocabulary::vocabulary;

/// The address a link points at, split as the reference's own writer splits it: the fragment becomes
/// the hyperlink's anchor when it is non-empty and fits Word's limit, and the target keeps the rest.
pub(super) struct Address {
    pub(super) target: String,
    pub(super) anchor: Option<String>,
}

/// The schemes a link may name: an application follows a target only when it can open one, and an
/// allowlist is the whole rule.
const FOLLOWABLE_SCHEMES: &[&str] = &["http", "https", "mailto", "tel"];

/// The longest fragment the reference writes as the hyperlink's own anchor.
const MAX_ANCHOR_LENGTH: usize = 255;

/// A link's address, checked where it is written: the address the parse produced is the one written,
/// and only a scheme an application can follow is accepted.
///
/// # Errors
///
/// The address does not parse as an absolute URL, or its scheme is not one of the four.
pub(super) fn link_address(url: &str) -> Result<Address, Failure> {
    let marks = &vocabulary().marks;
    let quoted = serde_json::to_string(url).expect("a string serialises");
    let Ok(parsed) = url::Url::parse(url) else {
        return Err(Failure::usage(
            marks.format("create_link_absolute", &[("url", &quoted)]),
        ));
    };
    if !FOLLOWABLE_SCHEMES.contains(&parsed.scheme()) {
        return Err(Failure::usage(
            marks.format("create_link_scheme", &[("url", &quoted)]),
        ));
    }
    let address = parsed.as_str();
    let index = address.find('#');
    let split = index.filter(|index| {
        let fragment = address.len() - index - 1;
        *index > 0 && fragment > 0 && fragment <= MAX_ANCHOR_LENGTH
    });
    Ok(match split {
        Some(index) => Address {
            target: address[..index].to_string(),
            anchor: Some(address[index + 1..].to_string()),
        },
        None => Address {
            target: address.to_string(),
            anchor: None,
        },
    })
}
