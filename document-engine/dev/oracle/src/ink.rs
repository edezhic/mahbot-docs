//! The ink rule and the faces a page draws: one walk of a page's content answers both.
//!
//! A page is blank when it carries **no ink**, and the rule is structural rather than a raster: a
//! raster would let the reference's own master background make an empty page look inked. Ink is:
//!
//! - a text-showing operator whose string holds a glyph other than a space, unless the current fill
//!   colour is white or the text rendering mode draws nothing;
//! - a path painted with a fill or a stroke, unless the fill colour is white, or the path covers at
//!   least `BACKGROUND_FRACTION` of the page, which is the reference's own background rather than
//!   anything the document draws;
//! - an image, a shading or an inline image placed on the page. The reference's background is
//!   always a fill, so no image is ever that background.
//!
//! A clip path is not ink, and neither is a marked-content boundary: the reference emits both on a
//! page it has drawn nothing on.
//!
//! A page whose only ink is a running header or a footer is therefore **not** blank, and a page the
//! reference's master background fills is blank whatever colour that background is.
//!
//! The same walk records the faces that **drew a glyph**: a text-showing operator that paints a
//! glyph is drawn in the face its `Tf` names, so a page that draws nothing lists no face, however
//! many its resources hold. That is the one walk of a page's content; the resources it needs — the
//! face each font name stands for and the content of each form — are collected once, before it.
//!
//! An image is always ink, because its extent is not known without tracking the page transform and
//! the reference draws its own background as a fill. A master background supplied as an image
//! rather than a fill would therefore make an empty page look inked; the writer cannot produce one,
//! and the corpus's deck states the reference's fill.
//!
//! # How the rule reads a page
//!
//! A fill needs a box with two non-zero extents, because a fill of a flat box paints nothing; a
//! stroke needs only a path that is not a single point, because a rule with no height is exactly
//! what a stroked line is. A line width of zero is a hairline and counts as ink; only a negative
//! width, which the format does not allow, paints nothing.
//!
//! The background test is stated for any painted path, so a path the document strokes around the
//! whole page would count as that background too. The reference never draws its background as a
//! stroke — it fills the page — so no page the corpus produces turns on the difference.
//!
//! Only a painting operator judges a path, and `n` ends one without painting it: the path a clip
//! names is discarded rather than carried into the next fill, so a clipped-to-page clip path cannot
//! make a small fill look like it covers the page.
//!
//! `q` and `Q` save and restore the colour, the line width and the text rendering mode, because the
//! reference sets its background's colour inside a saved state and the document's own colour
//! outside it: a rule that ignored the stack would read a black heading under a saved white fill
//! as white. A `cm` is not tracked: the reference states no page transform, so a path's coordinates
//! are already the page's own points.
//!
//! A content stream that cannot be decoded is an error, never a blank page: the run must fail
//! rather than record a page as empty because it could not be read. The stream is decoded
//! **strictly** for that reason — the lenient decoder answers the operators it managed to read,
//! and silently drops the rest, which is the same wrong answer one step further on.

use std::collections::{BTreeMap, BTreeSet};

use lopdf::content::{Content, Operation};
use lopdf::{Dictionary, Document, Object, ObjectId};

use crate::oracle::FORM_DEPTH;
use crate::oracle::pages::{dereference, dictionary, is_form, name, stream};

/// How much of a page a fill must cover to be the reference's background rather than the document's
/// ink: 99 per cent of its area.
const BACKGROUND_FRACTION: f64 = 0.99;

/// A form resolver: the content stream a `/XObject` name stands for, or `Ok(None)` for an image,
/// which counts as ink.
type Resolver<'a> = dyn FnMut(&[u8]) -> Result<Option<Vec<u8>>, String> + 'a;

/// How close a number must be to the value it is meant to equal: a content stream states `3` as an
/// integer or as a real, and both readings must agree.
const TOLERANCE: f64 = 1e-6;

/// What one page draws.
pub struct Drawn {
    /// Whether it carries any ink at all.
    pub ink: bool,
    /// The faces that drew a glyph, by PostScript name, sorted.
    pub faces: Vec<String>,
}

/// What one page of a PDF draws: whether it carries any ink, and the faces that drew a glyph.
///
/// # Errors
///
/// The page's content, its size or its resources cannot be read, its content stream cannot be
/// decoded, or its form `XObject`s are nested deeper than [`FORM_DEPTH`].
pub fn drawn(document: &Document, page: ObjectId) -> Result<Drawn, String> {
    let content = document
        .get_page_content(page)
        .map_err(|error| format!("the page's content cannot be read: {error}"))?;
    let (width, height) = crate::oracle::pages::size(document, page)?;
    let mut resources = Resources::default();
    if let Some(dictionary) = crate::oracle::pages::resources(document, page)? {
        collect(document, dictionary, 0, &mut resources)?;
    }
    let mut faces = BTreeSet::new();
    let mut form = |name: &[u8]| -> Result<Option<Vec<u8>>, String> {
        match resources.objects.get(name) {
            Some(Some(content)) => Ok(Some(content.clone())),
            Some(None) => Ok(None),
            // A name that resolves to nothing paints nothing: the reference emits such a name on a
            // page it drew nothing on, and reading it as an image would make that page look inked.
            None => Ok(Some(Vec::new())),
        }
    };
    let ink = has_ink(
        &content,
        width,
        height,
        &resources.fonts,
        &mut faces,
        &mut form,
    )?;
    Ok(Drawn {
        ink,
        faces: faces.into_iter().collect(),
    })
}

/// The resources a page and the forms it reaches name: the face each font name stands for, and the
/// content of each form `XObject` with `None` for an image.
///
/// The names are collected once per page rather than searched for each `Do`, and the first entry
/// for a name wins, so the page's own resources are preferred over a form's. A form's own
/// `/Resources` is followed; a form that states none is drawn with the resources it was placed by,
/// which are already being walked.
#[derive(Default)]
struct Resources {
    fonts: BTreeMap<Vec<u8>, String>,
    objects: BTreeMap<Vec<u8>, Option<Vec<u8>>>,
}

/// Collect what `resources` names, and what the forms it reaches name.
fn collect(
    document: &Document,
    resources: &Dictionary,
    depth: usize,
    out: &mut Resources,
) -> Result<(), String> {
    if depth > FORM_DEPTH {
        return Err(format!(
            "the page's form XObjects are nested deeper than {FORM_DEPTH} levels"
        ));
    }
    if let Ok(fonts) = resources.get(b"Font")
        && let Some(fonts) = dictionary(document, fonts)?
    {
        for (resource, font) in fonts {
            let Some(font) = dictionary(document, font)? else {
                continue;
            };
            let Ok(base) = font.get(b"BaseFont") else {
                continue;
            };
            if let Some(stated) = name(document, base)? {
                out.fonts
                    .entry(resource.clone())
                    .or_insert_with(|| strip_subset(&stated));
            }
        }
    }
    let Ok(xobjects) = resources.get(b"XObject") else {
        return Ok(());
    };
    let Some(xobjects) = dictionary(document, xobjects)? else {
        return Ok(());
    };
    // Every name this level states is inserted before any form is followed, so a form's own
    // resources can never take a name the page states: the page's names win, as documented.
    let mut forms = Vec::new();
    for (resource, xobject) in xobjects {
        if out.objects.contains_key(resource) {
            continue;
        }
        let Ok(xobject) = dereference(document, xobject) else {
            continue;
        };
        let Some(form) = stream(document, xobject)? else {
            continue;
        };
        if !is_form(form) {
            out.objects.insert(resource.clone(), None);
            continue;
        }
        let content = form
            .decompressed_content()
            .map_err(|error| format!("a form XObject's content cannot be read: {error}"))?;
        out.objects.insert(resource.clone(), Some(content));
        forms.push(form);
    }
    for form in forms {
        let Ok(own) = form.dict.get(b"Resources") else {
            continue;
        };
        if let Some(own) = dictionary(document, own)? {
            collect(document, own, depth + 1, out)?;
        }
    }
    Ok(())
}

/// A face's name without the six-letter tag the reference puts in front of a subset.
fn strip_subset(name: &str) -> String {
    let Some((tag, rest)) = name.split_once('+') else {
        return name.to_string();
    };
    if tag.len() == 6 && tag.bytes().all(|byte| byte.is_ascii_alphabetic()) {
        rest.to_string()
    } else {
        name.to_string()
    }
}

/// Whether a content stream paints any ink on a page `width` by `height` points.
///
/// `form` resolves a `/XObject` name to the content stream of a form, or answers `Ok(None)` for an
/// image, which counts as ink.
fn has_ink(
    content: &[u8],
    width: f64,
    height: f64,
    fonts: &BTreeMap<Vec<u8>, String>,
    faces: &mut BTreeSet<String>,
    form: &mut Resolver<'_>,
) -> Result<bool, String> {
    has_ink_within(content, width, height, fonts, faces, form, 0)
}

/// [`has_ink`], at `depth` levels of form nesting.
fn has_ink_within(
    content: &[u8],
    width: f64,
    height: f64,
    fonts: &BTreeMap<Vec<u8>, String>,
    faces: &mut BTreeSet<String>,
    form: &mut Resolver<'_>,
    depth: usize,
) -> Result<bool, String> {
    if depth > FORM_DEPTH {
        return Err(format!(
            "a form XObject's content is nested deeper than {FORM_DEPTH} levels"
        ));
    }
    let decoded = Content::decode_strict(content)
        .map_err(|error| format!("a content stream cannot be decoded: {error}"))?;
    let page_area = width * height;
    let mut ink = false;
    let mut state = State::default();
    let mut saved: Vec<State> = Vec::new();
    let mut path: Option<Bounds> = None;
    for operation in &decoded.operations {
        match operation.operator.as_str() {
            "m" | "l" => {
                if let Some(values) = numbers(operation, 2) {
                    include(&mut path, &values);
                }
            }
            "c" => {
                if let Some(values) = numbers(operation, 6) {
                    include(&mut path, &values);
                }
            }
            "v" | "y" => {
                if let Some(values) = numbers(operation, 4) {
                    include(&mut path, &values);
                }
            }
            "re" => {
                if let Some([x, y, across, down]) = numbers(operation, 4).as_deref() {
                    include(
                        &mut path,
                        &[*x, *y, x + across, *y, *x, y + down, x + across, y + down],
                    );
                }
            }
            "S" | "s" | "f" | "F" | "f*" | "B" | "B*" | "b" | "b*" => {
                let fills = matches!(
                    operation.operator.as_str(),
                    "f" | "F" | "f*" | "B" | "B*" | "b" | "b*"
                );
                let strokes = matches!(
                    operation.operator.as_str(),
                    "S" | "s" | "B" | "B*" | "b" | "b*"
                );
                if let Some(bounds) = path.take()
                    && paints(bounds, fills, strokes, &state, page_area)
                {
                    ink = true;
                }
            }
            "n" => path = None,
            "q" => saved.push(state.clone()),
            "Q" => {
                if let Some(restored) = saved.pop() {
                    state = restored;
                }
            }
            "g" => state.fill_is_white = is_white(numbers(operation, 1), 1.0),
            "rg" => state.fill_is_white = is_white(numbers(operation, 3), 1.0),
            "k" => state.fill_is_white = is_white(numbers(operation, 4), 0.0),
            "cs" | "sc" | "scn" => state.fill_is_white = false,
            "w" => {
                if let Some(values) = numbers(operation, 1) {
                    state.line_width = values[0];
                }
            }
            "Tr" => {
                if let Some(values) = numbers(operation, 1) {
                    state.render_mode = values[0];
                }
            }
            "Tf" => {
                state.font = operation
                    .operands
                    .first()
                    .and_then(|operand| operand.as_name().ok())
                    .map(<[u8]>::to_vec);
            }
            "Tj" | "TJ" | "'" | "\"" => {
                if shows_glyphs(operation, &state) {
                    ink = true;
                    if let Some(resource) = &state.font
                        && let Some(face) = fonts.get(resource)
                    {
                        faces.insert(face.clone());
                    }
                }
            }
            "Do" => {
                let Some(name) = operation
                    .operands
                    .first()
                    .and_then(|operand| operand.as_name().ok())
                else {
                    continue;
                };
                match form(name)? {
                    None => ink = true,
                    Some(content) => {
                        if has_ink_within(
                            &content,
                            width,
                            height,
                            fonts,
                            faces,
                            &mut *form,
                            depth + 1,
                        )? {
                            ink = true;
                        }
                    }
                }
            }
            "sh" | "BI" => ink = true,
            _ => {}
        }
    }
    Ok(ink)
}

/// The graphics state a painting operator is judged in.
#[derive(Clone, Debug)]
struct State {
    /// Whether the fill colour is white, which paints nothing visible on white paper.
    fill_is_white: bool,
    /// The line width, in points.
    line_width: f64,
    /// The text rendering mode.
    render_mode: f64,
    /// The font resource the text operators name, or `None` before any `Tf`.
    font: Option<Vec<u8>>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            // A page begins painting in black.
            fill_is_white: false,
            line_width: 1.0,
            render_mode: 0.0,
            font: None,
        }
    }
}

/// A path's bounding box, in the page's own coordinates.
#[derive(Clone, Copy, Debug)]
struct Bounds {
    /// The least x.
    min_x: f64,
    /// The least y.
    min_y: f64,
    /// The greatest x.
    max_x: f64,
    /// The greatest y.
    max_y: f64,
}

impl Bounds {
    /// Its width.
    fn width(self) -> f64 {
        self.max_x - self.min_x
    }

    /// Its height.
    fn height(self) -> f64 {
        self.max_y - self.min_y
    }

    /// Its area.
    fn area(self) -> f64 {
        self.width() * self.height()
    }
}

/// Grow a path's bounding box by the points an operator states, taken as x and y pairs.
fn include(path: &mut Option<Bounds>, points: &[f64]) {
    let mut grown = *path;
    let (pairs, _) = points.as_chunks::<2>();
    for &[x, y] in pairs {
        grown = Some(match grown {
            None => Bounds {
                min_x: x,
                min_y: y,
                max_x: x,
                max_y: y,
            },
            Some(bounds) => Bounds {
                min_x: bounds.min_x.min(x),
                min_y: bounds.min_y.min(y),
                max_x: bounds.max_x.max(x),
                max_y: bounds.max_y.max(y),
            },
        });
    }
    *path = grown;
}

/// Whether painting a path paints any ink.
fn paints(bounds: Bounds, fills: bool, strokes: bool, state: &State, page_area: f64) -> bool {
    if page_area > 0.0 && bounds.area() >= BACKGROUND_FRACTION * page_area {
        return false;
    }
    let fill_paints =
        fills && !state.fill_is_white && bounds.width() > 0.0 && bounds.height() > 0.0;
    let stroke_paints =
        strokes && state.line_width >= 0.0 && (bounds.width() > 0.0 || bounds.height() > 0.0);
    fill_paints || stroke_paints
}

/// Whether a text-showing operator paints a glyph.
fn shows_glyphs(operation: &Operation, state: &State) -> bool {
    let invisible =
        (state.render_mode - 3.0).abs() < TOLERANCE || (state.render_mode - 7.0).abs() < TOLERANCE;
    if state.fill_is_white || invisible {
        return false;
    }
    match operation.operator.as_str() {
        "TJ" => operation
            .operands
            .first()
            .and_then(|operand| operand.as_array().ok())
            .is_some_and(|items| items.iter().any(|item| item.as_str().is_ok_and(glyphs))),
        // `Tj` and `'` state the string alone; `"` states the word and character spacing first.
        _ => operation
            .operands
            .last()
            .and_then(|operand| operand.as_str().ok())
            .is_some_and(glyphs),
    }
}

/// Whether a shown string holds a glyph rather than only spacing.
fn glyphs(bytes: &[u8]) -> bool {
    bytes.iter().any(|byte| !matches!(byte, 0x20 | 0x09 | 0x00))
}

/// Whether a colour's components are all the extreme that white is stated as.
fn is_white(values: Option<Vec<f64>>, component: f64) -> bool {
    values.is_some_and(|values| {
        !values.is_empty()
            && values
                .iter()
                .all(|value| (value - component).abs() < TOLERANCE)
    })
}

/// The numbers an operation states, or `None` when it does not state exactly `count` of them.
fn numbers(operation: &Operation, count: usize) -> Option<Vec<f64>> {
    let values = operation
        .operands
        .iter()
        .map(|operand| match operand {
            Object::Integer(value) => i32::try_from(*value).ok().map(f64::from),
            Object::Real(value) => Some(f64::from(*value)),
            _ => None,
        })
        .collect::<Option<Vec<f64>>>()?;
    (values.len() == count).then_some(values)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The page the tests measure against: A4, in points, as the reference states it.
    const WIDTH: f64 = 595.0;
    const HEIGHT: f64 = 841.0;

    /// Whether `content` paints ink, with every `/XObject` name resolving to a form holding
    /// nothing.
    fn ink(content: &str) -> bool {
        ink_through(content, &mut |_| Ok(Some(Vec::new())))
    }

    /// Whether `content` paints ink, with `resolve` answering for every `/XObject` name.
    fn ink_through(
        content: &str,
        resolve: &mut impl FnMut(&[u8]) -> Result<Option<Vec<u8>>, String>,
    ) -> bool {
        let fonts = BTreeMap::new();
        let mut faces = BTreeSet::new();
        has_ink(
            content.as_bytes(),
            WIDTH,
            HEIGHT,
            &fonts,
            &mut faces,
            resolve,
        )
        .expect("a content stream a test states must decode")
    }

    /// The faces a content stream draws in, with every `/XObject` name resolving to a form holding
    /// nothing.
    fn faces(content: &str, fonts: &BTreeMap<Vec<u8>, String>) -> Vec<String> {
        let mut drawn = BTreeSet::new();
        has_ink(
            content.as_bytes(),
            WIDTH,
            HEIGHT,
            fonts,
            &mut drawn,
            &mut |_| Ok(Some(Vec::new())),
        )
        .expect("a content stream a test states must decode");
        drawn.into_iter().collect()
    }

    #[test]
    fn an_empty_content_stream_carries_no_ink() {
        assert!(!ink(""));
    }

    #[test]
    fn a_clip_path_is_not_ink() {
        assert!(!ink("q 0 0 595 841 re W* n Q"));
    }

    #[test]
    fn a_marked_content_boundary_is_not_ink() {
        assert!(!ink("/Artifact BMC EMC"));
    }

    #[test]
    fn a_text_run_carries_ink() {
        assert!(ink("BT /F1 12 Tf 10 10 Td (Hi) Tj ET"));
    }

    #[test]
    fn an_empty_text_run_carries_no_ink() {
        assert!(!ink("BT /F1 12 Tf 10 10 Td () Tj ET"));
        assert!(!ink("BT /F1 12 Tf 10 10 Td [( )] TJ ET"));
    }

    #[test]
    fn a_shown_space_carries_no_ink() {
        assert!(!ink("BT /F1 12 Tf 10 10 Td (   ) Tj ET"));
    }

    #[test]
    fn text_drawn_in_white_carries_no_ink() {
        assert!(!ink("1 1 1 rg BT /F1 12 Tf 10 10 Td (Hi) Tj ET"));
    }

    #[test]
    fn invisible_text_carries_no_ink() {
        assert!(!ink("BT 3 Tr /F1 12 Tf 10 10 Td (Hi) Tj ET"));
    }

    #[test]
    fn a_white_full_page_fill_is_the_background() {
        assert!(!ink("1 1 1 rg 0 0 595 841 re f"));
    }

    #[test]
    fn a_coloured_full_page_fill_is_the_background() {
        assert!(!ink("0.2 0.4 0.9 rg 0 0 595 841 re f"));
    }

    #[test]
    fn a_black_full_page_fill_is_the_background_too() {
        assert!(!ink("0 0 0 rg 0 0 595 841 re f"));
    }

    #[test]
    fn a_filled_rule_with_height_is_ink() {
        assert!(ink("0 0 0 rg 0 0 100 1 re f"));
    }

    #[test]
    fn a_filled_rule_of_zero_height_is_no_ink() {
        assert!(!ink("0 0 0 rg 0 0 100 0 re f"));
    }

    #[test]
    fn a_stroked_rule_is_ink() {
        assert!(ink("0.1 w 0 0 m 100 0 l S"));
    }

    #[test]
    fn a_path_that_is_never_painted_is_not_ink() {
        assert!(!ink("0 0 m 100 100 l"));
    }

    #[test]
    fn a_fill_after_a_clip_covers_only_its_own_path() {
        assert!(!ink("0 0 595 841 re W* n 0 0 10 0 re f"));
    }

    #[test]
    fn a_saved_white_fill_does_not_whiten_what_follows_it() {
        assert!(ink("q 1 1 1 rg Q BT /F1 12 Tf 10 10 Td (Hi) Tj ET"));
    }

    #[test]
    fn a_form_holding_text_carries_ink() {
        let mut resolve = |_: &[u8]| Ok(Some(b"BT /F1 12 Tf 10 10 Td (Hi) Tj ET".to_vec()));
        assert!(ink_through("/Fm0 Do", &mut resolve));
    }

    #[test]
    fn a_form_holding_nothing_carries_no_ink() {
        assert!(!ink("/Fm0 Do"));
    }

    #[test]
    fn an_image_carries_ink() {
        assert!(ink_through("/Im0 Do", &mut |_| Ok(None)));
    }

    #[test]
    fn a_form_that_reaches_itself_is_an_error() {
        let mut resolve = |_: &[u8]| Ok(Some(b"/Fm0 Do".to_vec()));
        let error = has_ink(
            b"/Fm0 Do",
            WIDTH,
            HEIGHT,
            &BTreeMap::new(),
            &mut BTreeSet::new(),
            &mut resolve,
        )
        .expect_err("a form that reaches itself must not be read");
        assert!(error.contains(&FORM_DEPTH.to_string()));
    }

    #[test]
    fn the_pages_own_name_wins_over_a_forms() {
        use lopdf::{Stream, dictionary};

        // The page and the form it reaches both name `/F1` and `/X0`; the page's `/F1` is another
        // face and its `/X0` is the form, where the form's own `/X0` is an image.
        let mut document = Document::with_version("1.5");
        let page_font =
            document.add_object(dictionary! { "Type" => "Font", "BaseFont" => "ArialMT" });
        let form_font =
            document.add_object(dictionary! { "Type" => "Font", "BaseFont" => "Times-Roman" });
        let image = document.add_object(Stream::new(
            dictionary! { "Type" => "XObject", "Subtype" => "Image" },
            Vec::new(),
        ));
        let form = document.add_object(Stream::new(
            dictionary! {
                "Type" => "XObject",
                "Subtype" => "Form",
                "Resources" => dictionary! {
                    "Font" => dictionary! { "F1" => form_font },
                    "XObject" => dictionary! { "X0" => image },
                },
            },
            Vec::new(),
        ));
        let page = dictionary! {
            "Font" => dictionary! { "F1" => page_font },
            "XObject" => dictionary! { "X0" => form },
        };
        let mut collected = Resources::default();
        collect(&document, &page, 0, &mut collected).expect("a resource tree a test states");
        assert_eq!(collected.fonts[&b"F1".to_vec()], "ArialMT");
        assert_eq!(collected.objects[&b"X0".to_vec()], Some(Vec::new()));
    }

    #[test]
    fn a_shading_carries_ink() {
        assert!(ink("/Sh0 sh"));
    }

    #[test]
    fn an_inline_image_carries_ink() {
        assert!(ink("BI /W 1 /H 1 /BPC 8 /CS /G ID \u{0} EI\n"));
    }

    #[test]
    fn an_undecodable_content_stream_is_an_error() {
        let error = has_ink(
            b"0 0 m ]",
            WIDTH,
            HEIGHT,
            &BTreeMap::new(),
            &mut BTreeSet::new(),
            &mut |_| Ok(None),
        )
        .expect_err("a content stream that does not decode must not be read as blank");
        assert!(error.contains("cannot be decoded"));
    }

    #[test]
    fn a_glyph_is_drawn_in_the_face_its_tf_names() {
        let fonts = [(b"F1".to_vec(), "LiberationSerif".to_string())]
            .into_iter()
            .collect();
        assert_eq!(
            faces("BT /F1 12 Tf 10 10 Td (Hi) Tj ET", &fonts),
            ["LiberationSerif"]
        );
    }

    #[test]
    fn a_page_that_draws_nothing_draws_in_no_face() {
        let fonts = [(b"F1".to_vec(), "LiberationSerif".to_string())]
            .into_iter()
            .collect();
        assert_eq!(
            faces("BT /F1 12 Tf 10 10 Td () Tj ET", &fonts),
            Vec::<String>::new()
        );
        assert_eq!(
            faces("q 0 0 595 841 re W* n Q", &fonts),
            Vec::<String>::new()
        );
    }

    #[test]
    fn text_drawn_in_white_draws_in_no_face() {
        let fonts = [(b"F1".to_vec(), "LiberationSerif".to_string())]
            .into_iter()
            .collect();
        assert_eq!(
            faces("1 1 1 rg BT /F1 12 Tf 10 10 Td (Hi) Tj ET", &fonts),
            Vec::<String>::new()
        );
    }

    #[test]
    fn a_subset_tag_is_stripped_from_a_face_name() {
        assert_eq!(strip_subset("BAAAAA+LiberationSerif"), "LiberationSerif");
    }

    #[test]
    fn a_face_name_without_a_subset_tag_is_kept() {
        assert_eq!(strip_subset("Times-Roman"), "Times-Roman");
        assert_eq!(strip_subset("ABC+Times"), "ABC+Times");
    }
}
