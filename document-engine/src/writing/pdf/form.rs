//! `pdf_form_fill`: substitute values keyed by field name, regenerate each field's appearance with
//! the embedded font, and optionally flatten the form into page content.
//!
//! A field is addressed by its fully-qualified name. A checkbox takes a boolean, a radio group and an
//! option list take one of their options, a dropdown takes a value of its own, and a text field takes
//! text; a name the form does not hold, or a field that cannot be filled, is refused outright with
//! the catalogue of what can be. The appearance of every field whose value changed (or whose
//! appearance the file does not carry) is regenerated, so a filled value is what a reader shows.
//!
//! A malformed form that leaves the reference's library without what it needs — a widget that states
//! no `/Rect`, or one whose appearance the file does not carry — is the engine's own failure here:
//! the class is `internal`, as the reference's is, but the sentence is the engine's rather than the
//! library's, so the corpus compares those cases by class alone. A widget whose `/P` names an object
//! that is not a page is not one of them: the reference falls back to the page whose annotation list
//! names the widget, and `widget_page` does the same.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::fmt::Write as _;

use lopdf::{Dictionary, Document, Object, ObjectId};

use super::draw;
use super::font::{Face, Subset};
use super::page;
use super::{Produced, editable};
use crate::operation::{Failure, Request};
use crate::vocabulary::{self, vocabulary};
use crate::writing::{js_truthy, scalar_value};

/// One terminal field: its object, the name it is addressed by, its kind and the widgets it draws.
struct Field {
    id: ObjectId,
    name: String,
    kind: Kind,
    widgets: Vec<ObjectId>,
    /// A text field's own `Multiline` flag: its value's line breaks are drawn as lines rather than
    /// merged into one.
    multiline: bool,
    /// A choice field's own `Sort` flag: its appearance lists the options in sorted order.
    sorted: bool,
    /// A text field's own `/MaxLen`: the longest value it accepts, in UTF-16 code units.
    max_length: Option<i64>,
    /// The cells a combed text field draws one character each: its own `/MaxLen`, present only
    /// when its `Comb` flag is set and it is neither multi-line nor a password or file selector.
    /// A field without `/MaxLen` is not combed, as the reference's own `isCombed` reads it.
    cells: Option<i64>,
}

/// The `Edit` flag of a choice field's `/Ff`: a dropdown accepts a value outside its own list, which
/// the reference turns on before it writes the value.
const EDIT_FLAG: i64 = 1 << 18;

/// The kinds a fill can meet.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Text,
    Checkbox,
    Radio,
    Dropdown,
    OptionList,
    PushButton,
    Signature,
    Unknown,
}

impl Kind {
    fn fillable(self) -> bool {
        matches!(
            self,
            Self::Text | Self::Checkbox | Self::Radio | Self::Dropdown | Self::OptionList
        )
    }

    /// The word the catalogue prints for this kind. Only a fillable kind is listed.
    fn word(self) -> &'static str {
        let kinds = vocabulary::pdf_field_kinds();
        match self {
            Self::Text => &kinds.text,
            Self::Checkbox => &kinds.checkbox,
            Self::Radio => &kinds.radio,
            Self::Dropdown | Self::OptionList => &kinds.choice,
            Self::PushButton | Self::Signature | Self::Unknown => {
                unreachable!("the catalogue lists only a fillable kind")
            }
        }
    }
}

/// Fill the request's form.
pub(super) fn run(request: &Request, produced: &mut Produced) -> Result<(), Failure> {
    // The operation surface refuses a request whose `values` is not an object before dispatch, so
    // this is the engine's own defect rather than a caller's condition.
    let Some(values) = request
        .values
        .get("values")
        .and_then(serde_json::Value::as_object)
    else {
        return Err(Failure::engine());
    };
    for (name, value) in values {
        if !scalar_value(value) {
            return Err(Failure::usage(
                vocabulary()
                    .marks
                    .format("surface_value_structure", &[("name", name)]),
            ));
        }
    }

    let page::Opened {
        mut document,
        pages: ids,
        chains,
        ..
    } = editable(request)?;
    // A file whose `/AcroForm` is not a dictionary is one the reference's library faults on while
    // it looks the form up, so it is the engine's own failure here rather than a field that is
    // missing from a form the file does not hold.
    if broken_acroform(&document, &chains) {
        return Err(Failure::engine());
    }
    let fields = parse(&mut document, &chains);
    let face = Face::bundled();
    let mut dirty = HashSet::new();
    let mut unsupported = draw::Unsupported::default();
    for (name, value) in values {
        let Some(field) = fields.iter().find(|field| &field.name == name) else {
            let shown = if name.is_empty() {
                "\"\"".to_string()
            } else {
                name.clone()
            };
            return Err(Failure::usage(vocabulary().marks.format(
                "pdf_form_no_field",
                &[
                    ("name", &shown),
                    ("catalogue", &catalogue(&document, &chains, &fields)?),
                ],
            )));
        };
        apply(&mut document, &chains, &fields, field, value)?;
        dirty.insert(field.id);
        if matches!(
            field.kind,
            Kind::Text | Kind::Radio | Kind::Dropdown | Kind::OptionList
        ) {
            unsupported.note(&string_of(value), face);
        }
    }

    // The layout each regenerated widget draws is computed once: the subset takes its characters,
    // and the appearance stream takes the same placed lines.
    let layouts = appearance_layouts(&document, &chains, &fields, &dirty, face)?;
    let characters: Vec<char> = layouts
        .iter()
        .flat_map(|(_, layout)| layout.lines.iter())
        .flat_map(|line| line.text.chars())
        .collect();
    let subset = face.subset(&characters);
    let font = draw::embed_font(&mut document, &subset, face);
    write_appearances(&mut document, &chains, &fields, font, &subset, &layouts)?;
    if request.values.get("flatten").is_some_and(js_truthy) {
        flatten(&mut document, &chains, &ids, &fields)?;
    }
    produced.push(&page::save(document, !ids.is_empty())?)?;
    produced.unsupported = unsupported;
    Ok(())
}

// ── the field tree ──────────────────────────────────────────────────────────────────────────────

/// Every terminal field, in field-tree order, with its direct-dictionary entries normalised to
/// object references so a field and its widgets can be written in place.
fn parse(document: &mut Document, chains: &page::Chains) -> Vec<Field> {
    let Some(acroform) = acroform(document) else {
        return Vec::new();
    };
    let fields = document
        .get_dictionary(acroform)
        .ok()
        .and_then(|dictionary| dictionary.get(b"Fields").ok().cloned());
    let Some(fields) = fields.and_then(|fields| normalize_array(document, &fields)) else {
        return Vec::new();
    };
    set_references(document, acroform, b"Fields", &fields);
    let mut out = Vec::new();
    for id in fields {
        visit(document, chains, id, "", &mut out, 0);
    }
    out
}

/// Whether the file states an `/AcroForm` that is not a dictionary.
fn broken_acroform(document: &Document, chains: &page::Chains) -> bool {
    let Some(root) = document
        .trailer
        .get(b"Root")
        .ok()
        .and_then(|root| root.as_reference().ok())
    else {
        return false;
    };
    let Some(value) = document
        .get_dictionary(root)
        .ok()
        .and_then(|catalog| catalog.get(b"AcroForm").ok())
    else {
        return false;
    };
    chains.resolve(document, value).as_dict().is_err()
}

/// The document's `/AcroForm`, as an object id: a direct dictionary is registered so its `/Fields`
/// can be rewritten.
fn acroform(document: &mut Document) -> Option<ObjectId> {
    let root = document.trailer.get(b"Root").ok()?.as_reference().ok()?;
    let value = document
        .get_dictionary(root)
        .ok()?
        .get(b"AcroForm")
        .ok()?
        .clone();
    match value {
        Object::Reference(id) => Some(id),
        Object::Dictionary(dictionary) => {
            let id = document.add_object(Object::Dictionary(dictionary));
            if let Ok(catalog) = document.get_object_mut(root).and_then(Object::as_dict_mut) {
                catalog.set("AcroForm", Object::Reference(id));
            }
            Some(id)
        }
        _ => None,
    }
}

/// An array's entries as object ids, registering any direct dictionary so it can be rewritten.
fn normalize_array(document: &mut Document, value: &Object) -> Option<Vec<ObjectId>> {
    let Object::Array(items) = value else {
        return None;
    };
    let mut ids = Vec::new();
    for item in items {
        match item {
            Object::Reference(id) => ids.push(*id),
            Object::Dictionary(dictionary) => {
                ids.push(document.add_object(Object::Dictionary(dictionary.clone())));
            }
            _ => {}
        }
    }
    Some(ids)
}

/// Store an array's entries as the references `normalize_array` answered, so a direct dictionary
/// the walk registered is the one later writes reach.
fn set_references(document: &mut Document, id: ObjectId, key: &[u8], entries: &[ObjectId]) {
    if let Ok(dictionary) = document.get_object_mut(id).and_then(Object::as_dict_mut) {
        dictionary.set(
            key,
            Object::Array(entries.iter().copied().map(Object::Reference).collect()),
        );
    }
}

/// Visit one field: a container recurses into the kids that name fields of their own, a terminal
/// field is recorded with its widgets.
fn visit(
    document: &mut Document,
    chains: &page::Chains,
    id: ObjectId,
    prefix: &str,
    out: &mut Vec<Field>,
    depth: usize,
) {
    if depth > vocabulary().limits.pdf_field_depth_max {
        return;
    }
    let Ok(dictionary) = document.get_dictionary(id).cloned() else {
        return;
    };
    let name = qualified_name(document, chains, id, prefix);
    let kids = dictionary
        .get(b"Kids")
        .ok()
        .and_then(|kids| normalize_array(document, kids))
        .unwrap_or_default();
    if !kids.is_empty() {
        set_references(document, id, b"Kids", &kids);
    }
    let container = kids
        .iter()
        .any(|kid| document.get_dictionary(*kid).is_ok_and(|kid| kid.has(b"T")));
    if container {
        for kid in kids {
            visit(document, chains, kid, &name, out, depth + 1);
        }
        return;
    }
    let widgets = if kids.is_empty() {
        vec![id]
    } else {
        kids.into_iter()
            .filter(|kid| {
                document
                    .get_dictionary(*kid)
                    .is_ok_and(|kid| !kid.has(b"T"))
            })
            .collect()
    };
    let kind = kind_of(document, chains, id);
    let flags = inherited(document, chains, id, b"Ff")
        .and_then(|flags| flags.as_i64().ok())
        .unwrap_or(0);
    // `/MaxLen` is the field's own entry, never inherited: the library reads it from the field
    // dictionary alone.
    let max_length = document
        .get_dictionary(id)
        .ok()
        .and_then(|dictionary| dictionary.get(b"MaxLen").ok())
        .and_then(|value| chains.resolve(document, value).as_i64().ok());
    let combed = kind == Kind::Text
        && flags & (1 << 24) != 0
        && flags & (1 << 12) == 0
        && flags & (1 << 13) == 0
        && flags & (1 << 20) == 0;
    out.push(Field {
        id,
        name,
        kind,
        widgets,
        multiline: kind == Kind::Text && flags & (1 << 12) != 0,
        sorted: matches!(kind, Kind::Dropdown | Kind::OptionList) && flags & (1 << 19) != 0,
        max_length,
        cells: max_length.filter(|_| combed),
    });
}

/// A field's fully-qualified name: its own `/T` under the prefix its ancestors established.
fn qualified_name(
    document: &Document,
    chains: &page::Chains,
    id: ObjectId,
    prefix: &str,
) -> String {
    let own = inherited(document, chains, id, b"T")
        .as_ref()
        .and_then(object_text)
        .unwrap_or_default();
    match (prefix.is_empty(), own.is_empty()) {
        (true, _) => own,
        (false, true) => prefix.to_string(),
        (false, false) => format!("{prefix}.{own}"),
    }
}

/// A field's kind, from its `/FT` and `/Ff` inherited along the `/Parent` chain.
fn kind_of(document: &Document, chains: &page::Chains, id: ObjectId) -> Kind {
    let Some(kind) = inherited(document, chains, id, b"FT") else {
        return Kind::Unknown;
    };
    let flags = inherited(document, chains, id, b"Ff")
        .and_then(|flags| flags.as_i64().ok())
        .unwrap_or(0);
    match kind.as_name().unwrap_or_default() {
        b"Tx" => Kind::Text,
        b"Ch" if flags & (1 << 17) != 0 => Kind::Dropdown,
        b"Ch" => Kind::OptionList,
        b"Btn" if flags & (1 << 15) != 0 => Kind::Radio,
        b"Btn" if flags & (1 << 16) != 0 => Kind::PushButton,
        b"Btn" => Kind::Checkbox,
        b"Sig" => Kind::Signature,
        _ => Kind::Unknown,
    }
}

/// One entry of a field, inherited along the `/Parent` chain and resolved.
fn inherited(
    document: &Document,
    chains: &page::Chains,
    id: ObjectId,
    key: &[u8],
) -> Option<Object> {
    let mut current = id;
    let mut seen = HashSet::new();
    while seen.insert(current) {
        let dictionary = document.get_dictionary(current).ok()?;
        if let Ok(value) = dictionary.get(key) {
            return Some(chains.resolve(document, value).clone());
        }
        current = dictionary
            .get(b"Parent")
            .ok()
            .and_then(|parent| parent.as_reference().ok())?;
    }
    None
}

/// The value a field holds, as text: a string decoded, a name as it stands.
fn value_of(document: &Document, chains: &page::Chains, id: ObjectId) -> Option<String> {
    inherited(document, chains, id, b"V")
        .as_ref()
        .and_then(object_text)
}

/// A PDF string or name as text: a string decoded (UTF-16BE with a byte-order mark, otherwise the
/// bytes as they stand), a name as it stands.
fn object_text(object: &Object) -> Option<String> {
    match object {
        Object::String(bytes, _) => Some(pdf_text(bytes)),
        Object::Name(name) => Some(String::from_utf8_lossy(name).into_owned()),
        _ => None,
    }
}

/// A PDF text string: UTF-16BE when it carries the byte-order mark, otherwise the bytes as they
/// stand.
pub(super) fn pdf_text(bytes: &[u8]) -> String {
    if let Some(rest) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        let units = rest
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| u16::from_be_bytes(*pair))
            .collect::<Vec<_>>();
        return String::from_utf16_lossy(&units);
    }
    String::from_utf8_lossy(bytes).into_owned()
}

/// A value as the kit's `String()` renders it.
fn string_of(value: &serde_json::Value) -> String {
    super::interpolated(Some(value))
}

// ── filling ─────────────────────────────────────────────────────────────────────────────────────

/// Apply one value to the field it names.
fn apply(
    document: &mut Document,
    chains: &page::Chains,
    fields: &[Field],
    field: &Field,
    value: &serde_json::Value,
) -> Result<(), Failure> {
    match field.kind {
        Kind::Checkbox => {
            let Some(checked) = value.as_bool() else {
                return Err(Failure::usage(vocabulary().marks.format(
                    "pdf_form_checkbox_value",
                    &[
                        ("name", &field.name),
                        ("value", &super::stringified(Some(value))),
                    ],
                )));
            };
            // The reference reads the on-state from the field's first widget alone, falling back to
            // `Yes` when that widget states none.
            let on = field
                .widgets
                .first()
                .and_then(|widget| on_value(document, chains, *widget))
                .unwrap_or_else(|| "Yes".to_string());
            let state = if checked { on } else { "Off".to_string() };
            set_value_name(document, field.id, &state);
            for widget in &field.widgets {
                let own = on_value(document, chains, *widget);
                let shown = if checked && own.as_deref() == Some(state.as_str()) {
                    state.clone()
                } else {
                    "Off".to_string()
                };
                set_name(document, *widget, "AS", &shown);
            }
        }
        Kind::Text => {
            let text = string_of(value);
            check_max_length(field, &text)?;
            if text.is_empty() {
                remove_key(document, field.id, b"V");
            } else {
                set_value_text(document, field.id, &text);
            }
        }
        Kind::Radio => {
            let (exports, states) = radio_options(document, chains, field)?;
            let options = exports.as_deref().unwrap_or(&states);
            let chosen = select(document, chains, fields, field, options, value)?;
            let state = radio_state(field, options, &states, &chosen)?;
            set_value_name(document, field.id, &state);
            for widget in &field.widgets {
                let own = on_value(document, chains, *widget).unwrap_or_default();
                let shown = if own == state { own } else { "Off".to_string() };
                set_name(document, *widget, "AS", &shown);
            }
        }
        Kind::OptionList => {
            let options = choice_options(document, chains, field)?;
            let chosen = select(document, chains, fields, field, &options, value)?;
            set_value_text(document, field.id, &chosen);
            // The reference's own `updateSelectedIndices` drops `/I` for the single value a fill
            // sets, so a filled list carries none.
            remove_key(document, field.id, b"I");
        }
        Kind::Dropdown => {
            let chosen = string_of(value);
            set_value_text(document, field.id, &chosen);
            // A value outside the field's own list turns editing on, as the reference's own
            // `select` does before it writes the value; a value that is one of the options leaves
            // the flag as the file states it.
            if !choice_options(document, chains, field)?.contains(&chosen) {
                set_flag(document, chains, field.id, EDIT_FLAG);
            }
            // The same `/I` rule as an option list's: one value carries no selected index.
            remove_key(document, field.id, b"I");
        }
        Kind::PushButton | Kind::Signature | Kind::Unknown => {
            return Err(Failure::usage(vocabulary().marks.format(
                "pdf_form_not_fillable",
                &[
                    ("name", &field.name),
                    ("catalogue", &catalogue(document, chains, fields)?),
                ],
            )));
        }
    }
    Ok(())
}

/// The library's own refusal of a text value past the field's stated `/MaxLen`, in its own words:
/// the kit reports it as a failure rather than as a refusal.
fn check_max_length(field: &Field, text: &str) -> Result<(), Failure> {
    let Some(max) = field.max_length else {
        return Ok(());
    };
    let length = utf16_length(text);
    if text.is_empty() || length <= max {
        return Ok(());
    }
    Err(Failure::internal(vocabulary().marks.format(
        "pdf_form_max_length",
        &[
            ("length", &length.to_string()),
            ("max", &max.to_string()),
            ("name", &field.name),
        ],
    )))
}

/// A string's length as the library counts it: UTF-16 code units, not characters.
fn utf16_length(text: &str) -> i64 {
    i64::try_from(text.encode_utf16().count()).unwrap_or(i64::MAX)
}

/// Set a flag bit on a field's own `/Ff`, keeping the flags the file already states.
fn set_flag(document: &mut Document, chains: &page::Chains, field: ObjectId, flag: i64) {
    let flags = inherited(document, chains, field, b"Ff")
        .and_then(|flags| flags.as_i64().ok())
        .unwrap_or(0);
    if let Ok(dictionary) = document.get_object_mut(field).and_then(Object::as_dict_mut) {
        dictionary.set("Ff", Object::Integer(flags | flag));
    }
}

/// The state a radio group's `/V` and widgets take for `chosen`: the on-state at the option's own
/// index. An option that outruns the on-states is the reference's own refusal.
fn radio_state(
    field: &Field,
    options: &[String],
    states: &[String],
    chosen: &str,
) -> Result<String, Failure> {
    let index = options
        .iter()
        .position(|option| option == chosen)
        .expect("select has already matched the value to one of the options");
    states.get(index).cloned().ok_or_else(|| {
        Failure::usage(vocabulary().marks.format(
            "pdf_form_option_failed",
            &[("name", &field.name), ("value", chosen)],
        ))
    })
}

/// A choice field's value: one of the options it offers, or a refusal naming what it takes.
fn select(
    document: &Document,
    chains: &page::Chains,
    fields: &[Field],
    field: &Field,
    options: &[String],
    value: &serde_json::Value,
) -> Result<String, Failure> {
    let chosen = string_of(value);
    if options.is_empty() {
        return Err(Failure::usage(vocabulary().marks.format(
            "pdf_form_no_choice",
            &[
                ("name", &field.name),
                ("catalogue", &catalogue(document, chains, fields)?),
            ],
        )));
    }
    if !options.contains(&chosen) {
        return Err(Failure::usage(vocabulary().marks.format(
            "pdf_form_option",
            &[
                ("name", &field.name),
                ("options", &options.join(", ")),
                ("value", &chosen),
            ],
        )));
    }
    Ok(chosen)
}

/// A radio group's own export values, when it states any, and the on-state each widget declares in
/// the same order: an option names the state at its own index, so a group that states no `/Opt` has
/// its on-states as its options.
fn radio_options(
    document: &Document,
    chains: &page::Chains,
    field: &Field,
) -> Result<(Option<Vec<String>>, Vec<String>), Failure> {
    let states = on_values(document, chains, field);
    Ok((export_values(document, chains, field)?, states))
}

/// A radio group's `/Opt` export values, as the reference's own library reads them: a string is one
/// value and an array contributes only its string entries, so a pair names no export value at all.
/// A field that states no `/Opt` has none to read, and one whose `/Opt` is of any other type is a
/// file the library faults on.
fn export_values(
    document: &Document,
    chains: &page::Chains,
    field: &Field,
) -> Result<Option<Vec<String>>, Failure> {
    let Some(opt) = inherited(document, chains, field.id, b"Opt") else {
        return Ok(None);
    };
    match opt {
        Object::Null => Ok(None),
        Object::String(bytes, _) => Ok(Some(vec![pdf_text(&bytes)])),
        Object::Array(items) => Ok(Some(
            items
                .iter()
                .filter_map(|item| opt_text(chains.resolve(document, item)))
                .collect(),
        )),
        _ => Err(Failure::engine()),
    }
}

/// The on-state each widget declares, in widget order: the states a radio group's `/Opt` entries
/// are matched to by index.
fn on_values(document: &Document, chains: &page::Chains, field: &Field) -> Vec<String> {
    field
        .widgets
        .iter()
        .filter_map(|widget| on_value(document, chains, *widget))
        .collect()
}

/// A choice field's options, as the reference's own library reads `/Opt`: a single string is one
/// option, a pair's display name is its second entry (its first when the pair states one entry or a
/// null one), an empty pair names nothing, and a top-level entry that is not text is skipped. A
/// `/Opt` or a pair entry of any other type is a file the library faults on, so it is the engine's
/// own failure here.
fn choice_options(
    document: &Document,
    chains: &page::Chains,
    field: &Field,
) -> Result<Vec<String>, Failure> {
    let mut options = Vec::new();
    match inherited(document, chains, field.id, b"Opt") {
        None | Some(Object::Null) => {}
        Some(opt @ Object::String(..)) => options.extend(opt_text(&opt)),
        Some(Object::Array(items)) => {
            for item in &items {
                match chains.resolve(document, item) {
                    Object::Array(pair) if !pair.is_empty() => {
                        let first = pair
                            .first()
                            .and_then(|entry| opt_text(chains.resolve(document, entry)))
                            .ok_or_else(Failure::engine)?;
                        // A pair's second entry names the display; an absent or null one leaves
                        // the first entry as both value and display.
                        let second = pair
                            .get(1)
                            .map(|entry| chains.resolve(document, entry))
                            .filter(|entry| !matches!(entry, Object::Null))
                            .map(|entry| opt_text(entry).ok_or_else(Failure::engine))
                            .transpose()?;
                        options.push(second.unwrap_or(first));
                    }
                    other => options.extend(opt_text(other)),
                }
            }
        }
        Some(_) => return Err(Failure::engine()),
    }
    Ok(options)
}

/// An `/Opt` entry's text: the library reads a string and skips anything else.
fn opt_text(object: &Object) -> Option<String> {
    match object {
        Object::String(bytes, _) => Some(pdf_text(bytes)),
        _ => None,
    }
}

/// The on-state a widget's normal appearance declares, or `None` when it declares none.
fn on_value(document: &Document, chains: &page::Chains, widget: ObjectId) -> Option<String> {
    let states = resolved_appearance(document, chains, widget)?;
    states
        .as_dict()
        .ok()?
        .iter()
        .find(|(key, _)| key.as_slice() != b"Off")
        .map(|(key, _)| String::from_utf8_lossy(key).into_owned())
}

// ── the catalogue ──────────────────────────────────────────────────────────────────────────────

/// The fields a fill can reach, as the refusal for a missing field lists them.
fn catalogue(
    document: &Document,
    chains: &page::Chains,
    fields: &[Field],
) -> Result<String, Failure> {
    let max = vocabulary().limits.pdf_catalogue_max;
    let chars_max = vocabulary().limits.pdf_catalogue_chars_max;
    let mut listed: Vec<String> = Vec::new();
    let mut truncated = false;
    for field in fields {
        if !field.kind.fillable() || field.name.is_empty() {
            continue;
        }
        let options = match field.kind {
            Kind::Radio => {
                let (exports, states) = radio_options(document, chains, field)?;
                exports.unwrap_or(states)
            }
            Kind::Dropdown | Kind::OptionList => choice_options(document, chains, field)?,
            _ => Vec::new(),
        };
        if matches!(field.kind, Kind::Radio | Kind::OptionList) && options.is_empty() {
            continue;
        }
        if listed.len() == max {
            truncated = true;
            break;
        }
        listed.push(describe(field, &options));
    }
    if listed.is_empty() {
        return Ok(vocabulary()
            .marks
            .get("pdf_form_catalogue_none")
            .to_string());
    }
    let mut catalogue = listed.join(", ");
    if truncated {
        catalogue.push_str(vocabulary().marks.get("pdf_form_catalogue_more"));
        return Ok(catalogue);
    }
    if catalogue.encode_utf16().count() > chars_max {
        // The reference's own `slice` counts UTF-16 code units, not characters.
        let units: Vec<u16> = catalogue.encode_utf16().take(chars_max).collect();
        catalogue = String::from_utf16_lossy(&units);
        catalogue.push('…');
    }
    Ok(catalogue)
}

/// One catalogue entry: the field's name and kind, and its options when it offers any.
fn describe(field: &Field, options: &[String]) -> String {
    if options.is_empty() {
        vocabulary().marks.format(
            "pdf_form_catalogue_field",
            &[("name", &field.name), ("kind", field.kind.word())],
        )
    } else {
        vocabulary().marks.format(
            "pdf_form_catalogue_options",
            &[
                ("name", &field.name),
                ("kind", field.kind.word()),
                ("options", &options.join(", ")),
            ],
        )
    }
}

// ── appearances ────────────────────────────────────────────────────────────────────────────────

/// One line of a regenerated appearance, where the reference's own layout draws it.
struct Placed {
    text: String,
    x: f64,
    y: f64,
}

/// The size and placed lines one widget's appearance draws, and the widget's own box.
struct Layout {
    size: f64,
    lines: Vec<Placed>,
    width: f64,
    height: f64,
}

/// Every regenerated text or choice widget's layout, in field-tree and widget order, and every push
/// button's caption. The subset and the appearance stream both read it, so the two passes cannot
/// disagree.
fn appearance_layouts(
    document: &Document,
    chains: &page::Chains,
    fields: &[Field],
    dirty: &HashSet<ObjectId>,
    face: &Face,
) -> Result<Vec<(ObjectId, Layout)>, Failure> {
    let mut layouts = Vec::new();
    for field in fields {
        if !regenerates(document, chains, field, dirty) {
            continue;
        }
        match field.kind {
            Kind::Text | Kind::Dropdown | Kind::OptionList => {
                let (source, multiline) = drawn_value(document, chains, field)?;
                for widget in &field.widgets {
                    layouts.push((
                        *widget,
                        layout_widget(document, chains, *widget, field, &source, multiline, face)?,
                    ));
                }
            }
            Kind::PushButton => {
                // A push button draws its widget's own `/MK /CA` caption; the reference regenerates
                // it whenever a widget's normal appearance is not a stream.
                for widget in &field.widgets {
                    let source =
                        vec![draw::lines_of(&caption(document, chains, *widget)).join(" ")];
                    layouts.push((
                        *widget,
                        layout_widget(document, chains, *widget, field, &source, false, face)?,
                    ));
                }
            }
            _ => {}
        }
    }
    Ok(layouts)
}

/// A push button's caption: its widget's `/MK /CA`, empty when the widget states none.
fn caption(document: &Document, chains: &page::Chains, widget: ObjectId) -> String {
    document
        .get_dictionary(widget)
        .ok()
        .and_then(|dictionary| dictionary.get(b"MK").ok())
        .and_then(|characteristics| chains.resolve(document, characteristics).as_dict().ok())
        .and_then(|characteristics| characteristics.get(b"CA").ok())
        .and_then(|value| object_text(chains.resolve(document, value)))
        .unwrap_or_default()
}

/// Whether this call regenerates the field's appearances: its value changed, or a widget's normal
/// appearance is not a stream the file already carries.
fn regenerates(
    document: &Document,
    chains: &page::Chains,
    field: &Field,
    dirty: &HashSet<ObjectId>,
) -> bool {
    dirty.contains(&field.id)
        || field.widgets.iter().any(|widget| {
            !matches!(
                resolved_appearance(document, chains, *widget),
                Some(Object::Stream(_))
            )
        })
}

/// The options an option list's appearance lists, sorted when the field says so.
fn listed_options(
    document: &Document,
    chains: &page::Chains,
    field: &Field,
) -> Result<Vec<String>, Failure> {
    let mut options = choice_options(document, chains, field)?;
    if field.sorted {
        options.sort();
    }
    Ok(options)
}

/// The lines a field's appearance starts from, and whether it lays them out as a multi-line field.
fn drawn_value(
    document: &Document,
    chains: &page::Chains,
    field: &Field,
) -> Result<(Vec<String>, bool), Failure> {
    let value = value_of(document, chains, field.id).unwrap_or_default();
    Ok(match field.kind {
        Kind::OptionList => (
            draw::lines_of(&listed_options(document, chains, field)?.join("\n")),
            true,
        ),
        Kind::Dropdown => (vec![draw::lines_of(&value).join(" ")], false),
        _ if field.multiline => (draw::lines_of(&value), true),
        _ => (vec![draw::lines_of(&value).join(" ")], false),
    })
}

/// The size and placed lines a widget's appearance draws: the size the widget or the field states
/// in its `/DA`, or the largest one the reference's own fitter chooses, then the value laid out in
/// the widget's own box.
///
/// A widget the library cannot place is its own failure rather than a widget the fill skips: the
/// reference reads the widget's rectangle before it lays anything out.
fn layout_widget(
    document: &Document,
    chains: &page::Chains,
    widget: ObjectId,
    field: &Field,
    source: &[String],
    multiline: bool,
    face: &Face,
) -> Result<Layout, Failure> {
    let dictionary = document
        .get_dictionary(widget)
        .map_err(|_| Failure::engine())?;
    let [x0, y0, x1, y1] =
        draw::rectangle(document, chains, dictionary).ok_or_else(Failure::engine)?;
    let (box_width, box_height) = (x1 - x0, y1 - y0);
    // The reference's own box: the border's width and, except for a combed field or a push button,
    // one point of padding on each side.
    let padding = if field.cells.is_some() || field.kind == Kind::PushButton {
        0.0
    } else {
        1.0
    };
    let inset = border_width(document, chains, dictionary) + padding;
    let width = box_width - 2.0 * inset;
    let height = box_height - 2.0 * inset;
    // The reference's combed fitter refuses a value wider than the cells it has.
    if let Some(cells) = field.cells {
        let length = source.first().map_or(0, |line| utf16_length(line));
        if length > cells {
            return Err(Failure::internal(vocabulary().marks.format(
                "pdf_form_combed_layout",
                &[
                    ("length", &length.to_string()),
                    ("cells", &cells.to_string()),
                ],
            )));
        }
    }
    let size = stated_size(document, chains, widget, field.id)
        .filter(|size| *size != 0.0)
        .unwrap_or_else(|| {
            if let Some(cells) = field.cells {
                combed_size(source, width, height, face, cells)
            } else {
                fit_size(source, width, height, face, multiline)
            }
        });
    // Only a text field reads its `/Q`: the reference's dropdown, option-list and push-button
    // providers state their own alignment.
    let alignment = match field.kind {
        Kind::Dropdown | Kind::OptionList => 0,
        Kind::PushButton => 1,
        _ => quadding(document, chains, field.id),
    };
    let lines = if let Some(cells) = field.cells {
        // One cell per character, each centred in its own cell.
        let count = f64::from(u32::try_from(cells.max(0)).unwrap_or(u32::MAX));
        let cell = width / count;
        let y = inset + (height / 2.0 - face.ascent(size) / 2.0);
        source
            .iter()
            .flat_map(|line| line.chars())
            .enumerate()
            .map(|(index, character)| {
                let text = character.to_string();
                Placed {
                    x: inset + cell * ordinal(index) + cell / 2.0 - face.width(&text, size) / 2.0,
                    y,
                    text,
                }
            })
            .collect()
    } else if multiline {
        let line_height = face.height(size) * LINE_HEIGHT_FACTOR;
        wrapped(source, size, width, face)
            .into_iter()
            .enumerate()
            .map(|(index, text)| Placed {
                x: inset + alignment_offset(alignment, width, face.width(&text, size)),
                y: inset + height - line_height * (ordinal(index) + 1.0),
                text,
            })
            .collect()
    } else {
        let y = inset + (height / 2.0 - face.ascent(size) / 2.0);
        source
            .iter()
            .map(|text| Placed {
                x: inset + alignment_offset(alignment, width, face.width(text, size)),
                y,
                text: text.clone(),
            })
            .collect()
    };
    Ok(Layout {
        size,
        lines,
        width: box_width,
        height: box_height,
    })
}

/// An index as the number an appearance stream writes.
fn ordinal(index: usize) -> f64 {
    f64::from(u32::try_from(index).unwrap_or(u32::MAX))
}

/// The width of the widget's own border: `/BS /W`, one point when `/BS` states no width, and none
/// when the widget states no `/BS`.
fn border_width(document: &Document, chains: &page::Chains, dictionary: &Dictionary) -> f64 {
    let Some(border) = dictionary.get(b"BS").ok() else {
        return 0.0;
    };
    let Object::Dictionary(border) = chains.resolve(document, border) else {
        return 0.0;
    };
    border
        .get(b"W")
        .ok()
        .and_then(|width| chains.resolve(document, width).as_float().ok())
        .map_or(1.0, f64::from)
}

/// The field's `/Q`: 0 left, 1 centred, 2 right, anything else left, as the reference reads it.
fn quadding(document: &Document, chains: &page::Chains, field: ObjectId) -> i64 {
    document
        .get_dictionary(field)
        .ok()
        .and_then(|dictionary| dictionary.get(b"Q").ok())
        .and_then(|value| chains.resolve(document, value).as_i64().ok())
        .unwrap_or(0)
}

/// Where a line `width` wide starts in a box `box_width` wide: 0 left, 1 centred, 2 right.
fn alignment_offset(alignment: i64, box_width: f64, width: f64) -> f64 {
    match alignment {
        1 => (box_width - width) / 2.0,
        2 => box_width - width,
        _ => 0.0,
    }
}

/// The largest size at which each character fits one cell of a combed field: three quarters of the
/// cell's width and the cell's whole height, as the reference's own combed fitter reads it.
fn combed_size(source: &[String], width: f64, height: f64, face: &Face, cells: i64) -> f64 {
    let cells = f64::from(u32::try_from(cells.max(0)).unwrap_or(u32::MAX));
    let cell_width = width / cells;
    let mut size = MIN_FONT_SIZE;
    while size < MAX_FONT_SIZE {
        let too_wide = source
            .iter()
            .flat_map(|line| line.chars())
            .any(|character| face.width(&character.to_string(), size) > cell_width * 0.75);
        if too_wide || face.ascent(size) > height {
            return size - 1.0;
        }
        size += 1.0;
    }
    size
}

/// The font size a widget or the field it belongs to states in its `/DA`, when it states one.
fn stated_size(
    document: &Document,
    chains: &page::Chains,
    widget: ObjectId,
    field: ObjectId,
) -> Option<f64> {
    let stated = |id| {
        document
            .get_dictionary(id)
            .ok()
            .and_then(|dictionary| dictionary.get(b"DA").ok())
            .and_then(|value| da_size(chains.resolve(document, value)))
    };
    stated(widget).or_else(|| stated(field))
}

/// The size a `/DA` string states: the number before the last `Tf`, as the reference reads it.
fn da_size(object: &Object) -> Option<f64> {
    let text = object_text(object)?;
    let tokens: Vec<&str> = text.split_whitespace().collect();
    tokens
        .iter()
        .enumerate()
        .rev()
        .find(|(index, token)| *index > 0 && **token == "Tf")
        .and_then(|(index, _)| tokens[index - 1].parse().ok())
}

/// Write every appearance the call regenerated, from the layouts the subset was taken from.
fn write_appearances(
    document: &mut Document,
    chains: &page::Chains,
    fields: &[Field],
    font: ObjectId,
    subset: &Subset,
    layouts: &[(ObjectId, Layout)],
) -> Result<(), Failure> {
    for (widget, layout) in layouts {
        let stream = text_appearance(document, layout, font, subset);
        set_normal_appearance(document, chains, *widget, Object::Reference(stream));
    }
    for field in fields {
        if !matches!(field.kind, Kind::Checkbox | Kind::Radio) {
            continue;
        }
        // A button's own appearance is the mark the file drew for its on-state; the value only
        // chooses which state shows, so an existing mark is kept. The reference regenerates one
        // whose normal appearance is not a state dictionary, or whose `/AS` names a state it does
        // not hold.
        for widget in &field.widgets {
            if button_needs_appearance(document, chains, *widget) {
                let on = on_value(document, chains, *widget).unwrap_or_else(|| "Yes".to_string());
                set_button_appearance(document, chains, *widget, &on)?;
            }
        }
    }
    Ok(())
}

/// Whether the reference regenerates a button's appearance: none is stated, the normal appearance
/// is not a state dictionary, or the widget's `/AS` names a state it does not hold.
fn button_needs_appearance(document: &Document, chains: &page::Chains, widget: ObjectId) -> bool {
    let Some(Object::Dictionary(states)) = resolved_appearance(document, chains, widget) else {
        return true;
    };
    let state = document
        .get_dictionary(widget)
        .ok()
        .and_then(|dictionary| dictionary.get(b"AS").ok())
        .and_then(|state| state.as_name().ok());
    state.is_some_and(|state| !states.has(state))
}

/// A widget's normal appearance as the file states it: the `/N` entry of its `/AP`, unresolved —
/// a stream's reference or a state dictionary, which are the two shapes the reference's own
/// `getNormalAppearance` accepts.
fn normal_appearance(
    document: &Document,
    chains: &page::Chains,
    widget: ObjectId,
) -> Option<Object> {
    let dictionary = document.get_dictionary(widget).ok()?;
    let appearances = dictionary.get(b"AP").ok()?;
    let appearances = chains.resolve(document, appearances).as_dict().ok()?;
    appearances.get(b"N").ok().cloned()
}

/// The same, resolved through its reference: the stream it names, or the state dictionary it is.
fn resolved_appearance(
    document: &Document,
    chains: &page::Chains,
    widget: ObjectId,
) -> Option<Object> {
    normal_appearance(document, chains, widget)
        .map(|normal| chains.resolve(document, &normal).clone())
}

/// Store a widget's normal appearance, keeping every other state it carried. The `/AP` dictionary
/// is resolved through its reference and written back where it lives, as the reference's own
/// `ensureAP` mutates it, so `/D`, `/R` and any other entry survive. An `/AP` that is not a
/// dictionary is replaced on the widget rather than overwritten where it points.
fn set_normal_appearance(
    document: &mut Document,
    chains: &page::Chains,
    widget: ObjectId,
    normal: Object,
) {
    let entry = document
        .get_dictionary(widget)
        .ok()
        .and_then(|dictionary| dictionary.get(b"AP").ok())
        .cloned();
    let resolved = entry.as_ref().and_then(|appearances| {
        chains
            .resolve(document, appearances)
            .as_dict()
            .ok()
            .cloned()
    });
    // The dictionary lives at the reference when there is one, and on the widget otherwise. An
    // `/AP` that resolves to something else is replaced on the widget, not overwritten where it
    // points.
    let target = match &entry {
        Some(Object::Reference(id)) if resolved.is_some() => Some(*id),
        _ => None,
    };
    let mut appearances = resolved.unwrap_or_default();
    appearances.set("N", normal);
    match target.and_then(|id| document.get_object_mut(id).ok()) {
        Some(slot) => *slot = Object::Dictionary(appearances),
        None => set_appearances(document, widget, appearances),
    }
}

/// Store a widget's `/AP` dictionary on the widget itself.
fn set_appearances(document: &mut Document, widget: ObjectId, appearances: Dictionary) {
    if let Ok(dictionary) = document
        .get_object_mut(widget)
        .and_then(Object::as_dict_mut)
    {
        dictionary.set("AP", Object::Dictionary(appearances));
    }
}

/// A text field's, choice field's or push-button caption's appearance: the lines the layout placed,
/// drawn inside the widget's own box with the embedded font the file states.
fn text_appearance(
    document: &mut Document,
    layout: &Layout,
    font: ObjectId,
    subset: &Subset,
) -> ObjectId {
    let mut content = format!(
        "q BT 0 g /{} {} Tf",
        draw::FONT_BASE,
        draw::number(layout.size)
    );
    for line in &layout.lines {
        let _ = write!(
            content,
            " 1 0 0 1 {} {} Tm <{}> Tj",
            draw::number(line.x),
            draw::number(line.y),
            draw::encode(subset, &line.text),
        );
    }
    content.push_str(" ET Q\n");
    let mut stream_dict = Dictionary::new();
    stream_dict.set("Type", draw::name("XObject"));
    stream_dict.set("Subtype", draw::name("Form"));
    stream_dict.set(
        "BBox",
        Object::Array(vec![
            Object::Integer(0),
            Object::Integer(0),
            draw::real(layout.width),
            draw::real(layout.height),
        ]),
    );
    stream_dict.set(
        "Matrix",
        Object::Array(
            [1, 0, 0, 1, 0, 0]
                .into_iter()
                .map(Object::Integer)
                .collect(),
        ),
    );
    let mut resources = Dictionary::new();
    let mut fonts = Dictionary::new();
    fonts.set(draw::FONT_BASE.as_bytes(), Object::Reference(font));
    resources.set("Font", Object::Dictionary(fonts));
    stream_dict.set("Resources", Object::Dictionary(resources));
    draw::stream(document, stream_dict, content.as_bytes())
}

/// The smallest and largest sizes the reference's own fitter tries.
const MIN_FONT_SIZE: f64 = 4.0;
const MAX_FONT_SIZE: f64 = 500.0;

/// The leading between a multi-line appearance's lines, as a multiple of the face's own height.
const LINE_HEIGHT_FACTOR: f64 = 1.2;

/// The largest size at which the value fits `width` by `height`, as the reference's own fitter
/// chooses it: an estimate from the words of each line rather than a re-wrap, so a single-line
/// value that fits word by word is drawn even when the whole line is wider than the box.
#[expect(
    clippy::cast_precision_loss,
    reason = "a line count is far below a double's exact range"
)]
fn fit_size(source: &[String], width: f64, height: f64, face: &Face, multiline: bool) -> f64 {
    let mut size = MIN_FONT_SIZE;
    while size < MAX_FONT_SIZE {
        let mut lines_used = 0_usize;
        for line in source {
            lines_used += 1;
            let words: Vec<&str> = line.split(' ').collect();
            let mut remaining = width;
            for (index, word) in words.iter().enumerate() {
                let word = if index + 1 == words.len() {
                    (*word).to_string()
                } else {
                    format!("{word} ")
                };
                let word_width = face.width(&word, size);
                remaining -= word_width;
                if remaining <= 0.0 {
                    lines_used += 1;
                    remaining = width - word_width;
                }
            }
        }
        if !multiline && lines_used > source.len() {
            return size - 1.0;
        }
        let line_height = face.height(size) * LINE_HEIGHT_FACTOR;
        if line_height * lines_used as f64 > height.abs() {
            return size - 1.0;
        }
        size += 1.0;
    }
    size
}

/// `source` wrapped to `width` at `size`, each piece split at the last whitespace that fits.
fn wrapped(source: &[String], size: f64, width: f64, face: &Face) -> Vec<String> {
    let mut out = Vec::new();
    for line in source {
        let mut current = line.clone();
        loop {
            let (piece, remainder) = split_line(&current, size, width, face);
            out.push(piece);
            match remainder {
                Some(rest) => current = rest,
                None => break,
            }
        }
    }
    out
}

/// The longest prefix of `line` that fits `width` at `size`, and the remainder past the whitespace
/// it broke at, trimmed as the reference trims it. A line with no whitespace that does not fit is
/// returned whole, and so is one whose only whitespace is its first character: the reference never
/// breaks before the first character.
///
/// One pass over the line: a width is a sum of advances, so the running sum is the same
/// measurement the reference takes of every shrinking prefix — and the sum stops at the first
/// character that overflows, so a wrapped value costs one pass per line rather than one per
/// character.
fn split_line(line: &str, size: f64, width: f64, face: &Face) -> (String, Option<String>) {
    let mut units = 0;
    let mut cut = None;
    for (index, character) in line.char_indices() {
        if index > 0 && character.is_whitespace() && face.scaled(units, size) < width {
            cut = Some(index);
        }
        units += face.units(character);
        if face.scaled(units, size) >= width {
            let Some(index) = cut else {
                return (line.to_string(), None);
            };
            let remainder = line[index..].trim();
            return (
                line[..index].to_string(),
                (!remainder.is_empty()).then(|| remainder.to_string()),
            );
        }
    }
    (line.to_string(), None)
}

/// A button's appearance: the on-state's mark and the empty off-state. A widget the library cannot
/// place — one that states no `/Rect`, or a rectangle that is not four numbers — is its own failure
/// rather than a mark drawn in a box the file never stated.
fn set_button_appearance(
    document: &mut Document,
    chains: &page::Chains,
    widget: ObjectId,
    on: &str,
) -> Result<(), Failure> {
    let dictionary = document
        .get_dictionary(widget)
        .map_err(|_| Failure::engine())?;
    let [x0, y0, x1, y1] =
        draw::rectangle(document, chains, dictionary).ok_or_else(Failure::engine)?;
    let (width, height) = (x1 - x0, y1 - y0);
    let bbox = || {
        Object::Array(vec![
            Object::Integer(0),
            Object::Integer(0),
            draw::real(width),
            draw::real(height),
        ])
    };
    let mut on_dict = Dictionary::new();
    on_dict.set("Type", draw::name("XObject"));
    on_dict.set("Subtype", draw::name("Form"));
    on_dict.set("BBox", bbox());
    let mark = format!(
        "q 0 0 0 rg {} {} {} {} re f Q",
        draw::number(width / 4.0),
        draw::number(height / 4.0),
        draw::number(width / 2.0),
        draw::number(height / 2.0),
    );
    let on_stream = draw::stream(document, on_dict, mark.as_bytes());
    let mut off_dict = Dictionary::new();
    off_dict.set("Type", draw::name("XObject"));
    off_dict.set("Subtype", draw::name("Form"));
    off_dict.set("BBox", bbox());
    let off_stream = draw::stream(document, off_dict, b"");
    let mut states = Dictionary::new();
    states.set(on.as_bytes(), Object::Reference(on_stream));
    states.set("Off", Object::Reference(off_stream));
    set_normal_appearance(document, chains, widget, Object::Dictionary(states));
    Ok(())
}

// ── flattening ─────────────────────────────────────────────────────────────────────────────────

/// Paint every widget's appearance into its page and remove the form from the file.
fn flatten(
    document: &mut Document,
    chains: &page::Chains,
    pages: &[ObjectId],
    fields: &[Field],
) -> Result<(), Failure> {
    // Every page a widget sits on states its own annotation array before a widget is removed from
    // it, as the reference's remove step does, so a widget listed through a reference is found
    // rather than painted and left in place. A page whose `/Annots` is not a list fails here, as
    // it does for `resize`.
    let mut normalised = BTreeSet::new();
    let mut content: HashMap<ObjectId, String> = HashMap::new();
    for field in fields {
        let mut drawn_on = BTreeSet::new();
        for widget in &field.widgets {
            // The reference resolves a widget's page and then the appearance it paints, and both
            // are its own failure when the file states neither: a widget it cannot place is not
            // one the flatten skips.
            let page_id =
                widget_page(document, chains, pages, *widget).ok_or_else(Failure::engine)?;
            let stream = selected_appearance(document, chains, *widget, field)?;
            if normalised.insert(page_id) {
                page::normalize_annots(document, chains, page_id)?;
            }
            drawn_on.insert(page_id);
            let key = draw::add_resource(document, page_id, "XObject", stream, draw::FLATTEN_BASE);
            let dictionary = document
                .get_dictionary(*widget)
                .map_err(|_| Failure::engine())?;
            let [x0, y0, ..] =
                draw::rectangle(document, chains, dictionary).ok_or_else(Failure::engine)?;
            // The reference translates to the widget's corner and then applies `rotateInPlace`'s
            // zero rotation: the two identity transforms after the translation are that turn.
            let _ = writeln!(
                content.entry(page_id).or_default(),
                "q 1 0 0 1 {} {} cm 1 0 0 1 0 0 cm 1 0 0 1 0 0 cm /{} Do Q",
                draw::number(x0),
                draw::number(y0),
                key,
            );
            remove_annot(document, page_id, *widget);
        }
        // The reference removes the field's own reference from every page that held one of its
        // widgets, so a field listed as an annotation beside its widgets leaves none behind.
        for page_id in drawn_on {
            remove_annot(document, page_id, field.id);
        }
    }
    for (page_id, content) in content {
        draw::push_content(document, page_id, content.as_bytes());
    }
    if let Some(acroform) = acroform_id(document)
        && let Ok(dictionary) = document
            .get_object_mut(acroform)
            .and_then(Object::as_dict_mut)
    {
        dictionary.set("Fields", Object::Array(Vec::new()));
    }
    Ok(())
}

/// The appearance stream a widget shows when it is flattened, as the reference resolves it: its
/// normal appearance, or — for a button whose normal appearance is a state dictionary — the state
/// the field's own `/V` names, falling back to `/Off`. Anything else is the library's own failure
/// rather than a widget the flatten skips: it resolves the appearance of every widget it paints.
fn selected_appearance(
    document: &Document,
    chains: &page::Chains,
    widget: ObjectId,
    field: &Field,
) -> Result<ObjectId, Failure> {
    let stream = match normal_appearance(document, chains, widget) {
        Some(Object::Reference(id)) => Some(id),
        // The library looks the state up once — the value's, or `/Off` when the value names none —
        // and requires what it finds to be a stream reference, so a state that is not one is a
        // failure rather than a reason to try the other.
        Some(Object::Dictionary(states)) if matches!(field.kind, Kind::Checkbox | Kind::Radio) => {
            let value = inherited(document, chains, field.id, b"V");
            let named = value.as_ref().and_then(|value| value.as_name().ok());
            [named, Some(b"Off".as_slice())]
                .into_iter()
                .flatten()
                .find_map(|state| states.get(state).ok())
                .and_then(|state| state.as_reference().ok())
        }
        _ => None,
    };
    stream.ok_or_else(Failure::engine)
}

/// The page a widget sits on: the one its `/P` names, when the document's page list holds it, or
/// else the one whose annotations list it. A `/P` naming anything else falls back to the
/// annotation search, as the reference's does.
fn widget_page(
    document: &Document,
    chains: &page::Chains,
    pages: &[ObjectId],
    widget: ObjectId,
) -> Option<ObjectId> {
    let named = document
        .get_dictionary(widget)
        .ok()
        .and_then(|dictionary| dictionary.get(b"P").ok())
        .and_then(|page| page.as_reference().ok())
        .filter(|page| pages.contains(page));
    if let Some(page) = named {
        return Some(page);
    }
    pages.iter().copied().find(|page| {
        document
            .get_dictionary(*page)
            .ok()
            .and_then(|dictionary| dictionary.get(b"Annots").ok())
            .and_then(|annots| chains.resolve(document, annots).as_array().ok())
            .is_some_and(|annots| {
                annots
                    .iter()
                    .any(|annot| annot.as_reference().ok() == Some(widget))
            })
    })
}

/// Remove one widget from a page's annotation list.
fn remove_annot(document: &mut Document, page: ObjectId, widget: ObjectId) {
    let Ok(dictionary) = document.get_object_mut(page).and_then(Object::as_dict_mut) else {
        return;
    };
    let Ok(annots) = dictionary.get(b"Annots") else {
        return;
    };
    let Ok(mut annots) = annots.as_array().cloned() else {
        return;
    };
    annots.retain(|annot| annot.as_reference().ok() != Some(widget));
    dictionary.set("Annots", Object::Array(annots));
}

/// The document's `/AcroForm` object, if the file states it as a reference.
fn acroform_id(document: &Document) -> Option<ObjectId> {
    let root = document.trailer.get(b"Root").ok()?.as_reference().ok()?;
    document
        .get_dictionary(root)
        .ok()?
        .get(b"AcroForm")
        .ok()?
        .as_reference()
        .ok()
}

// ── writing one field ─────────────────────────────────────────────────────────────────────────

/// Set a field's `/V` to a name.
fn set_value_name(document: &mut Document, id: ObjectId, value: &str) {
    set_key(document, id, b"V", draw::name(value));
}

/// Set a field's `/V` to text, as the UTF-16BE hex string the reference writes.
fn set_value_text(document: &mut Document, id: ObjectId, value: &str) {
    set_key(
        document,
        id,
        b"V",
        Object::String(utf16_be(value), lopdf::StringFormat::Hexadecimal),
    );
}

/// Set a name on one dictionary.
fn set_name(document: &mut Document, id: ObjectId, key: &str, value: &str) {
    set_key(document, id, key.as_bytes(), draw::name(value));
}

/// Set one key on one dictionary.
fn set_key(document: &mut Document, id: ObjectId, key: &[u8], value: Object) {
    if let Ok(dictionary) = document.get_object_mut(id).and_then(Object::as_dict_mut) {
        dictionary.set(key, value);
    }
}

/// Remove one key from one dictionary.
fn remove_key(document: &mut Document, id: ObjectId, key: &[u8]) {
    if let Ok(dictionary) = document.get_object_mut(id).and_then(Object::as_dict_mut) {
        dictionary.remove(key);
    }
}

/// A text as the format's UTF-16BE hex string: a byte-order mark, then two bytes a character.
fn utf16_be(text: &str) -> Vec<u8> {
    let mut bytes = vec![0xFE, 0xFF];
    for unit in text.encode_utf16() {
        bytes.extend_from_slice(&unit.to_be_bytes());
    }
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;
    use lopdf::{Document, StringFormat};

    /// A form whose fields are all text, named by `name`, in field-tree order.
    fn form_with_fields(count: usize, name: impl Fn(usize) -> String) -> Document {
        let mut document = Document::with_version("1.7");
        let mut fields = Vec::new();
        for index in 0..count {
            let mut field = Dictionary::new();
            field.set("FT", Object::Name(b"Tx".to_vec()));
            field.set(
                "T",
                Object::String(name(index).into_bytes(), StringFormat::Literal),
            );
            fields.push(Object::Reference(document.add_object(field)));
        }
        let mut acroform = Dictionary::new();
        acroform.set("Fields", Object::Array(fields));
        let acroform = document.add_object(acroform);
        let mut catalog = Dictionary::new();
        catalog.set("AcroForm", Object::Reference(acroform));
        let catalog = document.add_object(catalog);
        document.trailer.set("Root", Object::Reference(catalog));
        document
    }

    fn catalogue_of(document: &mut Document) -> String {
        let chains = page::Chains::of(document);
        let fields = parse(document, &chains);
        catalogue(document, &chains, &fields).expect("the test forms state no malformed option")
    }

    #[test]
    fn the_catalogue_lists_exactly_the_cap_and_says_more_only_past_it() {
        let max = vocabulary().limits.pdf_catalogue_max;
        let mut at_cap = form_with_fields(max, |index| format!("f{index}"));
        let exact = catalogue_of(&mut at_cap);
        assert!(
            !exact.ends_with(vocabulary().marks.get("pdf_form_catalogue_more")),
            "a form at the cap is not truncated"
        );
        let mut past = form_with_fields(max + 1, |index| format!("f{index}"));
        assert!(
            catalogue_of(&mut past).ends_with(vocabulary().marks.get("pdf_form_catalogue_more")),
            "a form past the cap is truncated"
        );
    }

    #[test]
    fn the_catalogue_caps_by_utf16_units_not_characters() {
        let max = vocabulary().limits.pdf_catalogue_max;
        let chars_max = vocabulary().limits.pdf_catalogue_chars_max;
        let mut document = form_with_fields(max, |_| "😀".to_string());
        let catalogue = catalogue_of(&mut document);
        assert_eq!(
            catalogue.encode_utf16().count(),
            chars_max + 1,
            "the cap is UTF-16 units plus the ellipsis"
        );
        assert!(
            catalogue.chars().count() < chars_max + 1,
            "an astral character counts as two units"
        );
    }
}
