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

use lopdf::{Dictionary, Object, ObjectId};

use super::graph::{self, Document};

use super::draw;
use super::font::{self, Face, Subset};
use super::page;
use super::{Produced, editable};
use crate::operation::{Failure, Request};
use crate::vocabulary::{self, vocabulary};
use crate::writing::{checked_values, entries, js_truthy};

/// One terminal field: its object, the name it is addressed by, its kind and the widgets it draws.
struct Field {
    id: ObjectId,
    name: String,
    kind: Kind,
    widgets: Vec<ObjectId>,
    /// Whether the reference's typed read of the field's widget list would throw. The reference reads
    /// that list after the values are applied, so the fault is raised there and not while the tree is
    /// built (`PARSER.md` §5.16).
    widget_fault: bool,
    /// A text field's own `/MaxLen`: the longest value it accepts, in UTF-16 code units.
    max_length: Option<i64>,
}

/// A text field's own appearance marks: the `Multiline` flag its value's line breaks are drawn by, and
/// the cells a combed field draws one character each — its `/MaxLen`, present only when the `Comb` flag
/// is set and it is neither multi-line nor a password or file selector, as the reference's own
/// `isCombed` reads it. Both come of the field's `/Ff`, which the reference reads only while it
/// regenerates that field's appearance (`PARSER.md` §5.16), so they are read where they are used.
#[derive(Default)]
struct Marks {
    multiline: bool,
    cells: Option<i64>,
}

/// The `Edit` flag of a choice field's `/Ff`: a dropdown accepts a value outside its own list, which
/// the reference turns on before it writes the value.
const EDIT_FLAG: i32 = 1 << 18;

/// The kinds a fill can meet.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Text,
    Checkbox,
    Radio,
    Dropdown,
    OptionList,
    PushButton,
    Signature,
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
            Self::PushButton | Self::Signature => {
                unreachable!("the catalogue lists only a fillable kind")
            }
        }
    }
}

/// Fill the request's form.
pub(super) fn run(request: &Request, produced: &mut Produced) -> Result<(), Failure> {
    // The reference reads `Object.entries(req.values)` twice — once to check each value's kind and
    // once to apply it, the second walk below. A shape with no entries (a number, a boolean)
    // produces the form unchanged, and a string or a list is indexed by the name its entry states.
    let values = checked_values(request)?;
    let values = entries(values);

    let page::Opened {
        mut document,
        pages: ids,
        ..
    } = editable(request, produced)?;
    // A file whose `/AcroForm` is not a dictionary is one the reference's library faults on while
    // it looks the form up, so it is the engine's own failure here rather than a field that is
    // missing from a form the file does not hold.
    if broken_acroform(&document) {
        return Err(Failure::engine());
    }
    // Before any value is applied, so a request that writes nothing drops the entry too.
    drop_xfa(&mut document);
    let flattened = request.values.get("flatten").is_some_and(js_truthy);
    let fields = parse(&mut document, flattened)?;
    let face = Face::bundled();
    let mut dirty = HashSet::new();
    let mut unsupported = draw::Unsupported::default();
    for (name, value) in &values {
        // The reference tests a requested name before it asks the form for a field, so an empty name
        // names nothing — not even a field the file states with an empty name.
        let wanted = (!name.is_empty()).then_some(name.as_str());
        let found = wanted.and_then(|name| fields.iter().find(|field| field.name == name));
        let Some(field) = found else {
            let shown = wanted.unwrap_or("\"\"");
            return Err(Failure::usage(vocabulary().marks.format(
                "pdf_form_no_field",
                &[
                    ("name", shown),
                    ("catalogue", &catalogue(&document, &fields)?),
                ],
            )));
        };
        apply(&mut document, &fields, field, value)?;
        dirty.insert(field.id);
        if matches!(
            field.kind,
            Kind::Text | Kind::Radio | Kind::Dropdown | Kind::OptionList
        ) {
            unsupported.note(&string_of(value), face);
        }
    }

    // The reference reads each field's widget list while it regenerates that field's appearance — after
    // the values are applied — so a list its typed read throws on is a fault there and here, never the
    // usage refusal a missing field is (`PARSER.md` §5.16).
    if fields.iter().any(|field| field.widget_fault) {
        return Err(Failure::engine());
    }

    // The layout each regenerated widget draws is computed once: the subset takes its characters,
    // and the appearance stream takes the same placed lines.
    let layouts = appearance_layouts(&document, &fields, &dirty, face)?;
    let characters: Vec<char> = layouts
        .iter()
        .flat_map(|(_, layout)| layout.lines.iter())
        .flat_map(|line| line.text.chars())
        .collect();
    let subset = face.subset(&characters);
    let font = draw::embed_font(&mut document, &subset, face);
    write_appearances(&mut document, &fields, font, &subset, &layouts)?;
    if flattened {
        flatten(&mut document, &ids, &fields)?;
    }
    produced.push(&page::save(document, !ids.is_empty())?)?;
    produced.unsupported = unsupported;
    Ok(())
}

// ── the field tree ──────────────────────────────────────────────────────────────────────────────

/// Every terminal field, in field-tree order, with its direct-dictionary entries normalised to
/// object references so a field and its widgets can be written in place.
///
/// The reference builds its field tree eagerly and reads every `/Fields` element with a typed
/// lookup, which throws for an entry that names nothing, names a further reference, or names a value
/// that is not a dictionary. Such a form is the engine's own failure here rather than a field
/// silently left out of the fill. `flatten` is the request's own flag, which decides whether a
/// signature's widgets are read (`widgets_of`).
fn parse(document: &mut Document, flatten: bool) -> Result<Vec<Field>, Failure> {
    let Some(acroform) = acroform(document) else {
        return Ok(Vec::new());
    };
    // `Fields()` is an untyped lookup: a value that is not an array is no list at all, and the entry is
    // left as the file states it (`PARSER.md` §5.16).
    let Listed::Array(items) = listed(document, acroform, b"Fields") else {
        return Ok(Vec::new());
    };
    let entries = register(document, items);
    // The one-step reader answers a dictionary for the entries the reference's typed lookup accepts. An
    // id it answers nothing for is an entry that lookup throws on: one that names nothing, names a
    // further reference, or names a value that is not a dictionary.
    if entries.broken
        || entries
            .ids
            .iter()
            .any(|id| graph::dictionary(document, *id).is_none())
    {
        return Err(Failure::engine());
    }
    if entries.registered {
        set_references(document, acroform, b"Fields", &entries.ids);
    }
    let mut out = Vec::new();
    walk(document, entries.ids, &mut out, flatten)?;
    Ok(out)
}

/// Whether the file states an `/AcroForm` that is not a dictionary.
fn broken_acroform(document: &Document) -> bool {
    let Some(root) = document
        .trailer
        .get(b"Root")
        .ok()
        .and_then(|root| root.as_reference().ok())
    else {
        return false;
    };
    let Some(value) =
        graph::dictionary(document, root).and_then(|catalog| catalog.get(b"AcroForm").ok())
    else {
        return false;
    };
    page::resolved(document, value).as_dict().is_err()
}

/// The document's `/AcroForm`, as an object id: a direct dictionary is registered so its `/Fields`
/// can be rewritten.
fn acroform(document: &mut Document) -> Option<ObjectId> {
    let root = document.trailer.get(b"Root").ok()?.as_reference().ok()?;
    let value = graph::clone(graph::dictionary(document, root)?.get(b"AcroForm").ok()?);
    match value {
        Object::Reference(id) => Some(id),
        Object::Dictionary(dictionary) => {
            let id = document.add_object(Object::Dictionary(dictionary));
            if let Some(catalog) = graph::dictionary_mut(document, root) {
                graph::set(catalog, b"AcroForm", Object::Reference(id));
            }
            Some(id)
        }
        // Nothing is kept, so the value is dismantled here: it may be nested as deep as the scan builds.
        other => {
            graph::discard(other);
            None
        }
    }
}

/// Drop the form's `/XFA` entry, as the reference's library does when it is asked for the form: a
/// hybrid file's packet would otherwise still name the original form, which a viewer that prefers
/// the packet shows instead of the fields just filled. The packet's own objects stay in the file,
/// unreferenced; nothing reads them. An entry whose value is `null` names nothing, and the
/// reference's own `has` reads it as absent and leaves it in place, so this does too.
fn drop_xfa(document: &mut Document) {
    if let Some(acroform) = acroform(document)
        && let Some(dictionary) = graph::dictionary_mut(document, acroform)
        && !matches!(dictionary.get(b"XFA"), Ok(Object::Null))
        && let Some(removed) = dictionary.remove(b"XFA")
    {
        graph::discard(removed);
    }
}

/// What one of the reference's one-step array lookups found.
#[derive(Default)]
enum Listed {
    /// No list: the entry is absent, states a `null`, or names nothing — `PDFDict.get` answers
    /// `undefined` for a stated null, and `lookupMaybe` does for a value that names nothing.
    #[default]
    Nothing,
    /// The array the entry states, or the array a reference it states names, as heap copies, because a
    /// caller registers or rewrites what it finds in them.
    Array(Vec<Object>),
    /// A value of another type, which a typed `lookupMaybe(…, PDFArray)` throws on and an untyped
    /// `lookup` reads as no list at all.
    Other,
}

/// `key`'s array as one of the reference's one-step lookups reads it.
fn listed(document: &Document, id: ObjectId, key: &[u8]) -> Listed {
    let Some(copy) = page::entry(document, id, key) else {
        return Listed::Nothing;
    };
    let answer = match page::resolved(document, &copy) {
        Object::Array(items) => Listed::Array(graph::clone_array(items)),
        Object::Null => Listed::Nothing,
        _ => Listed::Other,
    };
    graph::discard(copy);
    answer
}

/// What an array's entries name.
#[derive(Default)]
struct Entries {
    /// The object each entry names, in the order the array states them.
    ids: Vec<ObjectId>,
    /// A dictionary stated in place was registered, so the array has to be rewritten as the references
    /// the walk made.
    registered: bool,
    /// An entry was neither a reference nor a dictionary.
    broken: bool,
}

/// The ids an array's entries name, registering a dictionary the file stated in place so a later write
/// reaches the object it became. An entry that is neither is no id, for the caller to fault on or to
/// drop: `Fields.lookup(idx, PDFDict)` throws on it, `createPDFAcroFields` skips it.
fn register(document: &mut Document, items: Vec<Object>) -> Entries {
    let mut entries = Entries::default();
    for item in items {
        match item {
            Object::Reference(id) => entries.ids.push(id),
            Object::Dictionary(dictionary) => {
                entries
                    .ids
                    .push(document.add_object(Object::Dictionary(dictionary)));
                entries.registered = true;
            }
            other => {
                graph::discard(other);
                entries.broken = true;
            }
        }
    }
    entries
}

/// Store an array's entries as the references the walk answered, so a dictionary it registered — stated
/// in place where the file put it — is the one later writes reach.
fn set_references(document: &mut Document, id: ObjectId, key: &[u8], entries: &[ObjectId]) {
    if let Some(dictionary) = graph::dictionary_mut(document, id) {
        graph::set(
            dictionary,
            key,
            Object::Array(entries.iter().copied().map(Object::Reference).collect()),
        );
    }
}

/// One step of the field-tree walk: a field to visit, or a field whose kids are all visited and whose
/// place in the ancestor chain closes.
enum Step {
    Enter(ObjectId),
    Leave(ObjectId),
}

/// Every terminal field of the tree, in field-tree order, without recursing: a tree nests as deep as
/// the file states, and the language's own frames would end the process — not answer it — long before
/// a tree the reference fills (measured, the reference fills a 20 000-deep tree in under a second).
///
/// The walk keeps the chain of fields it is inside: a field that names one of them is a cycle and is
/// skipped, while one the file names twice under different parents is visited twice, as the
/// reference's walk visits it. The stop is this engine's own bound: measured, the reference's
/// `getAllFields` recurses on a non-terminal field and dies with `Maximum call stack size exceeded` on
/// the same shape (probe row "a form whose field kid list returns to its own container"), so a cycle
/// is a shape no answer here can match (`PARSER.md` §5.17).
fn walk(
    document: &mut Document,
    ids: Vec<ObjectId>,
    out: &mut Vec<Field>,
    flatten: bool,
) -> Result<(), Failure> {
    let mut path = HashSet::new();
    let mut work: Vec<Step> = ids.into_iter().rev().map(Step::Enter).collect();
    while let Some(step) = work.pop() {
        let id = match step {
            Step::Leave(id) => {
                path.remove(&id);
                continue;
            }
            Step::Enter(id) => id,
        };
        if !path.insert(id) {
            continue;
        }
        // The leave is pushed first, so it runs after every kid this field pushes above it.
        work.push(Step::Leave(id));
        let mut kids = listed(document, id, b"Kids");
        let outcome = field(document, id, out, &mut work, flatten, &mut kids);
        if let Listed::Array(items) = &mut kids {
            graph::discard_all(std::mem::take(items));
        }
        outcome?;
    }
    Ok(())
}

/// One field of the tree: a container hands its kids to the walk, a terminal field is recorded with its
/// widgets.
fn field(
    document: &mut Document,
    id: ObjectId,
    out: &mut Vec<Field>,
    work: &mut Vec<Step>,
    flatten: bool,
    kids: &mut Listed,
) -> Result<(), Failure> {
    if graph::dictionary(document, id).is_none() {
        return Ok(());
    }
    // `isNonTerminalAcroField`: a field whose `/Kids` states a kid that is a dictionary stating a `/T`
    // that is not null is a container. Its `/Kids` read is untyped, so a value that is not an array is
    // simply no kids. Nothing is registered for a field that turns out to be neither a container nor a
    // field of a known type.
    let container = match &*kids {
        Listed::Array(items) => items.iter().any(|kid| {
            graph::resolved(&document.objects, kid)
                .as_dict()
                .is_ok_and(states_a_name)
        }),
        _ => false,
    };
    if container {
        let Listed::Array(items) = std::mem::take(kids) else {
            return Ok(());
        };
        // `createPDFAcroFields` builds a field from a kid only when the array states a reference, so a
        // kid stated in place is no field of the reference's and the array is left as the file states it.
        // Pushed back to front, so the walk reaches them in the order the array states them.
        for kid in items.into_iter().rev() {
            match kid {
                Object::Reference(kid) => work.push(Step::Enter(kid)),
                other => graph::discard(other),
            }
        }
        return Ok(());
    }
    let Some(kind) = kind_of(document, id)? else {
        return Ok(());
    };
    let name = qualified_name(document, id)?;
    let (widgets, widget_fault) = match std::mem::take(kids) {
        // No `/Kids`: the field draws itself. An empty array is a list of no widgets, which is what the
        // reference's `getWidgets` answers for one.
        Listed::Nothing => (vec![id], false),
        Listed::Array(items) => widgets_of(document, id, items, kind, flatten),
        // `getWidgets` reads the list with a typed `lookupMaybe`, which throws on a value that is not an
        // array — where the appearance is regenerated, after the values are applied (`run`).
        Listed::Other => (Vec::new(), kind != Kind::Signature || flatten),
    };
    // `/MaxLen` is the field's own entry, never inherited: the library reads it from the field
    // dictionary alone.
    let max_length = graph::dictionary(document, id)
        .and_then(|dictionary| dictionary.get(b"MaxLen").ok())
        .and_then(|value| page::resolved(document, value).as_i64().ok());
    out.push(Field {
        id,
        name,
        kind,
        widgets,
        widget_fault,
        max_length,
    });
    Ok(())
}

/// Whether a dictionary states a `/T` that is not null — `PDFDict.has`, whose `get` answers `undefined`
/// for a stated null: the test that tells a field from a widget.
fn states_a_name(dictionary: &Dictionary) -> bool {
    !matches!(dictionary.get(b"T"), Ok(Object::Null) | Err(_))
}

/// The widgets a terminal field draws into: the dictionaries its `/Kids` array states, each dictionary
/// stated in place registered so its appearance can be written. A kid that states a name of its own is
/// a field, so the container test has already made this field a container and this function is not
/// called for it. The second answer is whether the reference's typed read of the list would throw: a
/// kid that is not a dictionary is that fault for every kind whose appearance a fill regenerates, and
/// for every field when the form is flattened. It is reported rather than raised, because the reference
/// reads the list after the values are applied (`run`).
fn widgets_of(
    document: &mut Document,
    id: ObjectId,
    items: Vec<Object>,
    kind: Kind,
    flatten: bool,
) -> (Vec<ObjectId>, bool) {
    let typed = kind != Kind::Signature || flatten;
    let entries = register(document, items);
    if entries.registered {
        set_references(document, id, b"Kids", &entries.ids);
    }
    let mut fault = entries.broken && typed;
    let mut widgets = Vec::new();
    for child in entries.ids {
        match graph::dictionary(document, child) {
            None => fault |= typed,
            Some(_) => widgets.push(child),
        }
    }
    (widgets, fault)
}

/// A field's fully-qualified name, as `getFullyQualifiedName` builds it: its own `/T` under the name
/// each link of its `/Parent` chain states, joined with `.`, and `undefined` for a link that states
/// none — the name a request that wants such a field has to ask for. The chain is read one step at a
/// time with the typed lookup `getParent` takes, so a link that names nothing, or names a value that
/// is not a dictionary, is the fault it throws. A chain that loops ends here: measured, the
/// reference's `getFullyQualifiedName` recurses on `/Parent` and dies with `Maximum call stack size
/// exceeded` on the same shape (probe row "a form whose field parent link returns to itself"), so the
/// stop is this engine's own bound (`PARSER.md` §5.17).
///
/// The divergences this reads — a `/T` stated as a name, and every name read while the tree is built
/// — are stated in `PARSER.md` §5.16.
fn qualified_name(document: &Document, id: ObjectId) -> Result<String, Failure> {
    let mut parts: Vec<Option<String>> = Vec::new();
    let mut seen = HashSet::new();
    let mut current = id;
    loop {
        if !seen.insert(current) {
            break;
        }
        let dictionary = graph::dictionary(document, current).ok_or_else(Failure::engine)?;
        parts.push(match dictionary.get(b"T") {
            // `T()` is a `lookupMaybe`, so an entry that is absent, is null, or names nothing is no
            // name at all rather than a fault.
            Ok(Object::Null) | Err(_) => None,
            Ok(value) => match page::resolved(document, value) {
                Object::Null => None,
                Object::String(bytes, _) => Some(pdf_text(bytes)),
                Object::Name(name) => Some(String::from_utf8_lossy(name).into_owned()),
                _ => return Err(Failure::engine()),
            },
        });
        let Some(parent) = dictionary
            .get(b"Parent")
            .ok()
            .and_then(|parent| parent.as_reference().ok())
        else {
            break;
        };
        if graph::dictionary(document, parent).is_none() {
            return Err(Failure::engine());
        }
        current = parent;
    }
    // `getName()` is `getFullyQualifiedName() ?? ""`: a field that states no name and has no parent of
    // its own has none, while a link of a longer chain that states none is JavaScript's `undefined`.
    if parts.len() == 1 && parts[0].is_none() {
        return Ok(String::new());
    }
    let unnamed = vocabulary().marks.get("edit_absent_value");
    Ok(parts
        .iter()
        .rev()
        .map(|part| part.as_deref().unwrap_or(unnamed))
        .collect::<Vec<_>>()
        .join("."))
}

/// A field's kind, from the `/FT` its chain states. `None` is a type the reference knows no field of —
/// the field it converts to nothing. The kind is decided by `/Ff` for a `/Ch` or a `/Btn`, and that
/// read is the constructor's; every later read of those flags takes the member ascent
/// (`PARSER.md` §5.16).
fn kind_of(document: &Document, id: ObjectId) -> Result<Option<Kind>, Failure> {
    let kind = typed_inherited(document, id, b"FT", Ascent::Constructor, |kind| {
        kind.as_name().ok().map(<[u8]>::to_vec)
    })?
    // `createPDFAcroTerminal` reads `/FT` with a lookup that has no absent case.
    .ok_or_else(Failure::engine)?;
    Ok(match kind.as_slice() {
        b"Tx" => Some(Kind::Text),
        b"Ch" => {
            let flags = flags_of(document, id, Ascent::Constructor)?;
            Some(if flags & (1 << 17) != 0 {
                Kind::Dropdown
            } else {
                Kind::OptionList
            })
        }
        b"Btn" => {
            let flags = flags_of(document, id, Ascent::Constructor)?;
            // `createPDFAcroButton` tests the push-button bit before the radio bit, so a field that
            // states both is a push button and no fill can take a value for it.
            Some(if flags & (1 << 16) != 0 {
                Kind::PushButton
            } else if flags & (1 << 15) != 0 {
                Kind::Radio
            } else {
                Kind::Checkbox
            })
        }
        b"Sig" => Some(Kind::Signature),
        // A terminal of another type is a `PDFAcroTerminal` the reference converts to no field at all:
        // it is not listed, and a value naming it is the refusal that names the fields it does hold.
        _ => None,
    })
}

/// A field's `/Ff`, inherited along the `/Parent` chain: the bit set that decides what a `/Btn` or a
/// `/Ch` field is, or that a text field's appearance is drawn by. Read with `lookupMaybe`, so a value
/// that names nothing is no flags at all.
///
/// The reference reads the entry as a number and tests it with JavaScript's `&`, which converts both
/// operands to a signed 32-bit integer: a negative real sets every bit, a value past 2^32 keeps only
/// its low bits, and a value no integer holds is zero. The flags are read the same way here.
#[expect(
    clippy::cast_precision_loss,
    reason = "the reference reads a whole number as the double its digits state, so the cast is the \
              conversion it takes"
)]
fn flags_of(document: &Document, id: ObjectId, ascent: Ascent) -> Result<i32, Failure> {
    Ok(
        maybe_inherited(document, id, b"Ff", ascent, |flags| match flags {
            // `PDFNumber` holds a whole number as the double its digits state, so an integer is read
            // through that double before the 32-bit conversion.
            Object::Integer(flags) => Some(to_int32(*flags as f64)),
            Object::Real(flags) => Some(to_int32(f64::from(*flags))),
            _ => None,
        })?
        .unwrap_or(0),
    )
}

/// JavaScript's `ToInt32`, the conversion the reference's `&` applies to a field's flags: a value that
/// is not finite is zero, the fraction is truncated toward zero, and the result is the low 32 bits as a
/// signed integer. The reference reads a decimal as a double; an integer is exact in both models, while
/// a real has already been rounded to this model's 32-bit float (`PARSER.md` §5.16).
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap,
    reason = "the value is reduced into [0, 2^32), so the cast states its low 32 bits as the signed \
              integer JavaScript's `&` uses"
)]
fn to_int32(value: f64) -> i32 {
    if !value.is_finite() {
        return 0;
    }
    // The width of a 32-bit integer: `ToInt32` keeps the value modulo 2^32.
    let bits = value.trunc().rem_euclid(f64::from(u32::MAX) + 1.0) as u32;
    bits as i32
}

/// How far an entry's ascent follows the `/Parent` chain. The reference reads a field's inherited
/// entries with three different readers, and the difference decides what a field inherits
/// (`PARSER.md` §5.16).
#[derive(Clone, Copy)]
enum Ascent {
    /// The field constructor's `ascend`, which reads each link with `lookupMaybe(…, PDFDict)`: a
    /// dictionary stated in place is followed, a link that names nothing ends the chain, and one that
    /// names an existing value that is not a dictionary is the fault the lookup throws.
    Constructor,
    /// `getParent`, which every later read of an inherited entry ascends through: only a reference that
    /// names a dictionary is followed — a dictionary stated in place is no parent — and a link that
    /// names nothing is the fault its typed `lookup` throws.
    Member,
    /// No ascent at all: `PDFAcroChoice.Opt` reads the field's own dictionary, so a value its parent
    /// states is none of its own.
    Own,
}

/// The value a field's entry states, from the first dictionary up its `/Parent` chain that states it:
/// the raw value, as the reference's own inheritance reads it — one step through the object map, never
/// a chain. The value is copied on the heap and dismantled here: one a file states may be nested as
/// deep as the scan builds one, so it is never dropped by the language.
///
/// A stated `null` is no value (`PDFDict.get` answers `undefined` for one, so the search carries on up
/// the chain), and a chain that loops ends here: measured, the reference's constructor `ascend`
/// recurses on `/Parent` and dies with `Maximum call stack size exceeded` on the same shape (probe row
/// "a form whose field parent link returns to itself"), so the stop is this engine's own bound
/// (`PARSER.md` §5.17).
fn inherited_value(
    document: &Document,
    id: ObjectId,
    key: &[u8],
    ascent: Ascent,
) -> Result<Option<Object>, Failure> {
    let mut node = graph::dictionary(document, id);
    let mut seen = HashSet::from([id]);
    while let Some(dictionary) = node {
        if let Ok(value) = dictionary.get(key)
            && !matches!(value, Object::Null)
        {
            return Ok(Some(graph::clone(page::resolved(document, value))));
        }
        if matches!(ascent, Ascent::Own) {
            break;
        }
        node = match dictionary.get(b"Parent") {
            // No link, or one that states a `null`: `PDFDict.get` answers `undefined` for both, and
            // both ascents stop there.
            Err(_) | Ok(Object::Null) => None,
            // The constructor's `lookupMaybe(…, PDFDict)` follows a dictionary stated in place;
            // `getParent` follows a reference only.
            Ok(Object::Dictionary(parent)) if matches!(ascent, Ascent::Constructor) => Some(parent),
            Ok(Object::Reference(parent)) => {
                match (seen.insert(*parent), document.objects.get(parent)) {
                    (false, _) => None,
                    (_, Some(Object::Dictionary(parent))) => Some(parent),
                    // `getParent`'s typed `lookup` throws on a link that names nothing or names a
                    // value that is not a dictionary; the constructor's read ends at the first.
                    (_, None) if matches!(ascent, Ascent::Constructor) => None,
                    (_, _) => return Err(Failure::engine()),
                }
            }
            Ok(_) if matches!(ascent, Ascent::Constructor) => return Err(Failure::engine()),
            Ok(_) => None,
        };
    }
    Ok(None)
}

/// Run `read` over a field's entry, inherited along the `/Parent` chain, for a reader that answers the
/// value itself rather than a type: `/V`, whose read is untyped so no value of it is a fault, and
/// `/Opt`, whose reader decides which values of it the reference faults on.
fn with_inherited<T>(
    document: &Document,
    id: ObjectId,
    key: &[u8],
    ascent: Ascent,
    read: impl FnOnce(&Object) -> T,
) -> Result<Option<T>, Failure> {
    let Some(value) = inherited_value(document, id, key, ascent)? else {
        return Ok(None);
    };
    let answer = read(&value);
    graph::discard(value);
    Ok(Some(answer))
}

/// [`with_inherited`] for an entry the reference reads with `lookupMaybe`, which answers no value for
/// one that names nothing and throws for one of another type (`/Ff`).
fn maybe_inherited<T>(
    document: &Document,
    id: ObjectId,
    key: &[u8],
    ascent: Ascent,
    read: impl FnOnce(&Object) -> Option<T>,
) -> Result<Option<T>, Failure> {
    let Some(value) = inherited_value(document, id, key, ascent)? else {
        return Ok(None);
    };
    let answer = match &value {
        Object::Null => Ok(None),
        object => read(object).map_or_else(|| Err(Failure::engine()), |value| Ok(Some(value))),
    };
    graph::discard(value);
    answer
}

/// [`with_inherited`] for an entry the reference reads with a typed `lookup`, which throws for a value
/// that names nothing as well as for one of another type (`/FT`).
fn typed_inherited<T>(
    document: &Document,
    id: ObjectId,
    key: &[u8],
    ascent: Ascent,
    read: impl FnOnce(&Object) -> Option<T>,
) -> Result<Option<T>, Failure> {
    let Some(value) = inherited_value(document, id, key, ascent)? else {
        return Ok(None);
    };
    let answer = read(&value);
    graph::discard(value);
    answer.map_or_else(|| Err(Failure::engine()), |value| Ok(Some(value)))
}

/// The value a field holds, as text: a string decoded, a name as it stands. `/V` is read with an
/// untyped lookup, so no value of it is a fault.
fn value_of(document: &Document, id: ObjectId) -> Result<Option<String>, Failure> {
    Ok(with_inherited(document, id, b"V", Ascent::Member, object_text)?.flatten())
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
                .and_then(|widget| on_value(document, *widget))
                .unwrap_or_else(|| "Yes".to_string());
            let state = if checked { on } else { "Off".to_string() };
            set_value_name(document, field.id, &state);
            for widget in &field.widgets {
                let own = on_value(document, *widget);
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
            let (exports, states) = radio_options(document, field)?;
            let options = exports.as_deref().unwrap_or(&states);
            let chosen = select(document, fields, field, options, value)?;
            let state = radio_state(field, options, &states, &chosen)?;
            set_value_name(document, field.id, &state);
            for widget in &field.widgets {
                let own = on_value(document, *widget).unwrap_or_default();
                let shown = if own == state { own } else { "Off".to_string() };
                set_name(document, *widget, "AS", &shown);
            }
        }
        Kind::OptionList => {
            let options = choice_options(document, field)?;
            let chosen = select(document, fields, field, &options, value)?;
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
            if !choice_options(document, field)?.contains(&chosen) {
                set_flag(document, field.id, EDIT_FLAG)?;
            }
            // The same `/I` rule as an option list's: one value carries no selected index.
            remove_key(document, field.id, b"I");
        }
        Kind::PushButton | Kind::Signature => {
            return Err(Failure::usage(vocabulary().marks.format(
                "pdf_form_not_fillable",
                &[
                    ("name", &field.name),
                    ("catalogue", &catalogue(document, fields)?),
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
fn set_flag(document: &mut Document, field: ObjectId, flag: i32) -> Result<(), Failure> {
    let flags = flags_of(document, field, Ascent::Member)?;
    if let Some(dictionary) = graph::dictionary_mut(document, field) {
        // `setFlag` writes the 32-bit result, so a file that stated its flags as a real or past a
        // 32-bit integer gets an integer of those bits.
        graph::set(dictionary, b"Ff", Object::Integer(i64::from(flags | flag)));
    }
    Ok(())
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
                ("catalogue", &catalogue(document, fields)?),
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
    field: &Field,
) -> Result<(Option<Vec<String>>, Vec<String>), Failure> {
    let states = on_values(document, field);
    Ok((export_values(document, field)?, states))
}

/// A radio group's `/Opt` export values, as the reference's own library reads them: a string is one
/// value and an array contributes only its string entries, so a pair names no export value at all.
/// A field that states no `/Opt` has none to read, and one whose `/Opt` is of any other type is a
/// file the library faults on. `Opt()` reads the field's own dictionary, so a value its parent states
/// is none of its own.
fn export_values(document: &Document, field: &Field) -> Result<Option<Vec<String>>, Failure> {
    let values = with_inherited(document, field.id, b"Opt", Ascent::Own, |opt| match opt {
        Object::Null => Ok(None),
        Object::String(bytes, _) => Ok(Some(vec![pdf_text(bytes)])),
        Object::Array(items) => Ok(Some(
            items
                .iter()
                .filter_map(|item| opt_text(page::resolved(document, item)))
                .collect(),
        )),
        _ => Err(Failure::engine()),
    })?;
    values.unwrap_or(Ok(None))
}

/// The on-state each widget declares, in widget order: the states a radio group's `/Opt` entries
/// are matched to by index.
fn on_values(document: &Document, field: &Field) -> Vec<String> {
    field
        .widgets
        .iter()
        .filter_map(|widget| on_value(document, *widget))
        .collect()
}

/// A choice field's options, as the reference's own library reads `/Opt`: a single string is one
/// option, a pair's display name is its second entry (its first when the pair states one entry or a
/// null one), an empty pair names nothing, and a top-level entry that is not text is skipped. A
/// `/Opt` or a pair entry of any other type is a file the library faults on, so it is the engine's
/// own failure here. `Opt()` reads the field's own dictionary, so a value its parent states is none
/// of its own.
fn choice_options(document: &Document, field: &Field) -> Result<Vec<String>, Failure> {
    let options = with_inherited(document, field.id, b"Opt", Ascent::Own, |opt| {
        let mut options = Vec::new();
        match opt {
            Object::Null => {}
            Object::String(..) => options.extend(opt_text(opt)),
            Object::Array(items) => {
                for item in items {
                    match page::resolved(document, item) {
                        Object::Array(pair) if !pair.is_empty() => {
                            let first = pair
                                .first()
                                .and_then(|entry| opt_text(page::resolved(document, entry)))
                                .ok_or_else(Failure::engine)?;
                            // A pair's second entry names the display; an absent or null one leaves
                            // the first entry as both value and display.
                            let second = pair
                                .get(1)
                                .map(|entry| page::resolved(document, entry))
                                .filter(|entry| !matches!(entry, Object::Null))
                                .map(|entry| opt_text(entry).ok_or_else(Failure::engine))
                                .transpose()?;
                            options.push(second.unwrap_or(first));
                        }
                        other => options.extend(opt_text(other)),
                    }
                }
            }
            _ => return Err(Failure::engine()),
        }
        Ok(options)
    })?;
    options.unwrap_or(Ok(Vec::new()))
}

/// An `/Opt` entry's text: the library reads a string and skips anything else.
fn opt_text(object: &Object) -> Option<String> {
    match object {
        Object::String(bytes, _) => Some(pdf_text(bytes)),
        _ => None,
    }
}

/// The on-state a widget's normal appearance declares, or `None` when it declares none.
fn on_value(document: &Document, widget: ObjectId) -> Option<String> {
    with_resolved_appearance(document, widget, |states| {
        states
            .as_dict()
            .ok()?
            .iter()
            .find(|(key, _)| key.as_slice() != b"Off")
            .map(|(key, _)| String::from_utf8_lossy(key).into_owned())
    })
    .flatten()
}

// ── the catalogue ──────────────────────────────────────────────────────────────────────────────

/// The fields a fill can reach, as the refusal for a missing field lists them.
fn catalogue(document: &Document, fields: &[Field]) -> Result<String, Failure> {
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
                let (exports, states) = radio_options(document, field)?;
                exports.unwrap_or(states)
            }
            Kind::Dropdown | Kind::OptionList => choice_options(document, field)?,
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
    fields: &[Field],
    dirty: &HashSet<ObjectId>,
    face: &Face,
) -> Result<Vec<(ObjectId, Layout)>, Failure> {
    let mut layouts = Vec::new();
    for field in fields {
        if !regenerates(document, field, dirty) {
            continue;
        }
        match field.kind {
            Kind::Text | Kind::Dropdown | Kind::OptionList => {
                // A text field's `/Ff` is read here, where the reference reads it; a choice's decided
                // the kind already.
                let marks = match field.kind {
                    Kind::Text => marks_of(document, field)?,
                    _ => Marks::default(),
                };
                let (source, multiline) = drawn_value(document, field, &marks)?;
                for widget in &field.widgets {
                    layouts.push((
                        *widget,
                        layout_widget(document, *widget, field, &marks, &source, multiline, face)?,
                    ));
                }
            }
            Kind::PushButton => {
                // A push button draws its widget's own `/MK /CA` caption; the reference regenerates
                // it whenever a widget's normal appearance is not a stream.
                let marks = Marks::default();
                for widget in &field.widgets {
                    let source = vec![draw::lines_of(&caption(document, *widget)).join(" ")];
                    layouts.push((
                        *widget,
                        layout_widget(document, *widget, field, &marks, &source, false, face)?,
                    ));
                }
            }
            _ => {}
        }
    }
    Ok(layouts)
}

/// A text field's appearance marks, read from its `/Ff` where the reference reads it: while it
/// regenerates that field's appearance, not while the tree is built (`PARSER.md` §5.16).
fn marks_of(document: &Document, field: &Field) -> Result<Marks, Failure> {
    let flags = flags_of(document, field.id, Ascent::Member)?;
    let multiline = flags & (1 << 12) != 0;
    let combed =
        flags & (1 << 24) != 0 && !multiline && flags & (1 << 13) == 0 && flags & (1 << 20) == 0;
    Ok(Marks {
        multiline,
        cells: field.max_length.filter(|_| combed),
    })
}

/// A push button's caption: its widget's `/MK /CA`, empty when the widget states none.
fn caption(document: &Document, widget: ObjectId) -> String {
    graph::dictionary(document, widget)
        .and_then(|dictionary| dictionary.get(b"MK").ok())
        .and_then(|characteristics| page::resolved(document, characteristics).as_dict().ok())
        .and_then(|characteristics| characteristics.get(b"CA").ok())
        .and_then(|value| object_text(page::resolved(document, value)))
        .unwrap_or_default()
}

/// Whether this call regenerates the field's appearances: its value changed, or a widget's normal
/// appearance is not a stream the file already carries.
fn regenerates(document: &Document, field: &Field, dirty: &HashSet<ObjectId>) -> bool {
    dirty.contains(&field.id)
        || field.widgets.iter().any(|widget| {
            !with_resolved_appearance(document, *widget, |appearance| {
                matches!(appearance, Object::Stream(_))
            })
            .unwrap_or(false)
        })
}

/// The options an option list's appearance lists, sorted when the field says so. `isSorted` reads the
/// `Sort` bit through `getFlags`, which inherits `/Ff` with the member ascent — a later read than the
/// one that decides the field's kind (`PARSER.md` §5.16).
fn listed_options(document: &Document, field: &Field) -> Result<Vec<String>, Failure> {
    let mut options = choice_options(document, field)?;
    if flags_of(document, field.id, Ascent::Member)? & (1 << 19) != 0 {
        options.sort();
    }
    Ok(options)
}

/// The lines a field's appearance starts from, and whether it lays them out as a multi-line field.
fn drawn_value(
    document: &Document,
    field: &Field,
    marks: &Marks,
) -> Result<(Vec<String>, bool), Failure> {
    let value = value_of(document, field.id)?.unwrap_or_default();
    Ok(match field.kind {
        Kind::OptionList => (
            draw::lines_of(&listed_options(document, field)?.join("\n")),
            true,
        ),
        Kind::Dropdown => (vec![draw::lines_of(&value).join(" ")], false),
        _ if marks.multiline => (draw::lines_of(&value), true),
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
    widget: ObjectId,
    field: &Field,
    marks: &Marks,
    source: &[String],
    multiline: bool,
    face: &Face,
) -> Result<Layout, Failure> {
    let dictionary = graph::dictionary(document, widget).ok_or_else(Failure::engine)?;
    let [x0, y0, x1, y1] = draw::rectangle(document, dictionary).ok_or_else(Failure::engine)?;
    let (box_width, box_height) = (x1 - x0, y1 - y0);
    // The reference's own box: the border's width and, except for a combed field or a push button,
    // one point of padding on each side.
    let padding = if marks.cells.is_some() || field.kind == Kind::PushButton {
        0.0
    } else {
        1.0
    };
    let inset = border_width(document, dictionary) + padding;
    let width = box_width - 2.0 * inset;
    let height = box_height - 2.0 * inset;
    // The reference's combed fitter refuses a value wider than the cells it has.
    if let Some(cells) = marks.cells {
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
    let size = stated_size(document, widget, field.id)
        .filter(|size| *size != 0.0)
        .unwrap_or_else(|| {
            if let Some(cells) = marks.cells {
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
        _ => quadding(document, field.id),
    };
    let lines = if let Some(cells) = marks.cells {
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
fn border_width(document: &Document, dictionary: &Dictionary) -> f64 {
    let Some(border) = dictionary.get(b"BS").ok() else {
        return 0.0;
    };
    let Object::Dictionary(border) = page::resolved(document, border) else {
        return 0.0;
    };
    border
        .get(b"W")
        .ok()
        .and_then(|width| page::resolved(document, width).as_float().ok())
        .map_or(1.0, f64::from)
}

/// The field's `/Q`: 0 left, 1 centred, 2 right, anything else left, as the reference reads it.
fn quadding(document: &Document, field: ObjectId) -> i64 {
    graph::dictionary(document, field)
        .and_then(|dictionary| dictionary.get(b"Q").ok())
        .and_then(|value| page::resolved(document, value).as_i64().ok())
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
fn stated_size(document: &Document, widget: ObjectId, field: ObjectId) -> Option<f64> {
    let stated = |id| {
        graph::dictionary(document, id)
            .and_then(|dictionary| dictionary.get(b"DA").ok())
            .and_then(|value| da_size(page::resolved(document, value)))
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
    fields: &[Field],
    font: ObjectId,
    subset: &Subset,
    layouts: &[(ObjectId, Layout)],
) -> Result<(), Failure> {
    for (widget, layout) in layouts {
        let stream = text_appearance(document, layout, font, subset);
        set_normal_appearance(document, *widget, Object::Reference(stream));
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
            if button_needs_appearance(document, *widget) {
                let on = on_value(document, *widget).unwrap_or_else(|| "Yes".to_string());
                set_button_appearance(document, *widget, &on)?;
            }
        }
    }
    Ok(())
}

/// Whether the reference regenerates a button's appearance: none is stated, the normal appearance
/// is not a state dictionary, or the widget's `/AS` names a state it does not hold.
fn button_needs_appearance(document: &Document, widget: ObjectId) -> bool {
    with_resolved_appearance(document, widget, |appearance| {
        let Object::Dictionary(states) = appearance else {
            return true;
        };
        let state = graph::dictionary(document, widget)
            .and_then(|dictionary| dictionary.get(b"AS").ok())
            .and_then(|state| state.as_name().ok());
        state.is_some_and(|state| !states.has(state))
    })
    .unwrap_or(true)
}

/// A widget's normal appearance as the file states it, read by `read` and dismantled here: the `/N`
/// entry of its `/AP`, unresolved — a stream's reference or a state dictionary, which are the two
/// shapes the reference's own `getNormalAppearance` accepts. The entry may be nested as deep as the
/// scan builds, so it is copied on the heap and never dropped by the language.
fn with_normal_appearance<T>(
    document: &Document,
    widget: ObjectId,
    read: impl FnOnce(&Object) -> T,
) -> Option<T> {
    let dictionary = graph::dictionary(document, widget)?;
    let appearances = dictionary.get(b"AP").ok()?;
    let appearances = page::resolved(document, appearances).as_dict().ok()?;
    let normal = graph::clone(appearances.get(b"N").ok()?);
    let answer = read(&normal);
    graph::discard(normal);
    Some(answer)
}

/// The same, resolved through its reference: `read` sees the stream it names, or the state dictionary
/// it is. The unresolved entry is dismantled by [`with_normal_appearance`], so nothing it holds is
/// dropped by the language's own recursion.
fn with_resolved_appearance<T>(
    document: &Document,
    widget: ObjectId,
    read: impl FnOnce(&Object) -> T,
) -> Option<T> {
    with_normal_appearance(document, widget, |normal| {
        read(page::resolved(document, normal))
    })
}

/// Store a widget's normal appearance, keeping every other state it carried. The `/AP` dictionary
/// is resolved through its reference and written back where it lives, as the reference's own
/// `ensureAP` mutates it, so `/D`, `/R` and any other entry survive. An `/AP` that is not a
/// dictionary is replaced on the widget rather than overwritten where it points.
fn set_normal_appearance(document: &mut Document, widget: ObjectId, normal: Object) {
    let entry = graph::dictionary(document, widget)
        .and_then(|dictionary| dictionary.get(b"AP").ok())
        .map(graph::clone);
    let resolved = entry.as_ref().and_then(|appearances| {
        page::resolved(document, appearances)
            .as_dict()
            .ok()
            .map(graph::clone_dictionary)
    });

    // The dictionary lives at the reference when there is one, and on the widget otherwise. An
    // `/AP` that resolves to something else is replaced on the widget, not overwritten where it
    // points.
    let target = match &entry {
        Some(Object::Reference(id)) if resolved.is_some() => Some(*id),
        _ => None,
    };
    let mut appearances = resolved.unwrap_or_default();
    graph::set(&mut appearances, b"N", normal);
    match target.and_then(|id| document.objects.get_mut(&id)) {
        Some(slot) => {
            graph::discard(std::mem::replace(slot, Object::Dictionary(appearances)));
        }
        None => set_appearances(document, widget, appearances),
    }
    // The entry is read by its shape rather than kept, so it is dismantled here: it may be nested as
    // deep as the scan builds.
    if let Some(entry) = entry {
        graph::discard(entry);
    }
}

/// Store a widget's `/AP` dictionary on the widget itself.
fn set_appearances(document: &mut Document, widget: ObjectId, appearances: Dictionary) {
    match graph::dictionary_mut(document, widget) {
        Some(dictionary) => graph::set(dictionary, b"AP", Object::Dictionary(appearances)),
        // The dictionary was built here and may hold a value nested as deep as the scan builds one,
        // so it is dismantled rather than dropped by the language.
        None => graph::discard(Object::Dictionary(appearances)),
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
    let mut content = format!("q BT 0 g /{} {} Tf", font::NAME, draw::number(layout.size));
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
    let fonts = Dictionary::from_iter([(font::NAME, Object::Reference(font))]);
    let stream_dict = Dictionary::from_iter([
        ("Type", draw::name("XObject")),
        ("Subtype", draw::name("Form")),
        (
            "BBox",
            Object::Array(vec![
                Object::Integer(0),
                Object::Integer(0),
                draw::real(layout.width),
                draw::real(layout.height),
            ]),
        ),
        (
            "Matrix",
            Object::Array(
                [1, 0, 0, 1, 0, 0]
                    .into_iter()
                    .map(Object::Integer)
                    .collect(),
            ),
        ),
        (
            "Resources",
            Object::Dictionary(Dictionary::from_iter([("Font", Object::Dictionary(fonts))])),
        ),
    ]);
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
    widget: ObjectId,
    on: &str,
) -> Result<(), Failure> {
    let dictionary = graph::dictionary(document, widget).ok_or_else(Failure::engine)?;
    let [x0, y0, x1, y1] = draw::rectangle(document, dictionary).ok_or_else(Failure::engine)?;
    let (width, height) = (x1 - x0, y1 - y0);
    let bbox = || {
        Object::Array(vec![
            Object::Integer(0),
            Object::Integer(0),
            draw::real(width),
            draw::real(height),
        ])
    };
    let form = || {
        Dictionary::from_iter([
            ("Type".to_string(), draw::name("XObject")),
            ("Subtype".to_string(), draw::name("Form")),
            ("BBox".to_string(), bbox()),
        ])
    };
    let on_dict = form();
    let mark = format!(
        "q 0 0 0 rg {} {} {} {} re f Q",
        draw::number(width / 4.0),
        draw::number(height / 4.0),
        draw::number(width / 2.0),
        draw::number(height / 2.0),
    );
    let on_stream = draw::stream(document, on_dict, mark.as_bytes());
    let off_stream = draw::stream(document, form(), b"");
    let states = Dictionary::from_iter([
        (on.to_string(), Object::Reference(on_stream)),
        ("Off".to_string(), Object::Reference(off_stream)),
    ]);
    set_normal_appearance(document, widget, Object::Dictionary(states));
    Ok(())
}

// ── flattening ─────────────────────────────────────────────────────────────────────────────────

/// Paint every widget's appearance into its page and remove the form from the file.
fn flatten(document: &mut Document, pages: &[ObjectId], fields: &[Field]) -> Result<(), Failure> {
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
            let page_id = widget_page(document, pages, *widget).ok_or_else(Failure::engine)?;
            let stream = selected_appearance(document, *widget, field)?;
            if normalised.insert(page_id) {
                page::normalize_annots(document, page_id)?;
            }
            drawn_on.insert(page_id);
            let key = draw::add_resource(document, page_id, "XObject", stream, draw::FLATTEN_BASE)?;
            let dictionary = graph::dictionary(document, *widget).ok_or_else(Failure::engine)?;
            let [x0, y0, ..] = draw::rectangle(document, dictionary).ok_or_else(Failure::engine)?;
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
        draw::push_content(document, page_id, content.as_bytes())?;
    }
    if let Some(acroform) = acroform_id(document)
        && let Some(dictionary) = graph::dictionary_mut(document, acroform)
    {
        graph::set(dictionary, b"Fields", Object::Array(Vec::new()));
    }
    Ok(())
}

/// The appearance stream a widget shows when it is flattened, as the reference resolves it: its
/// normal appearance, or — for a button whose normal appearance is a state dictionary — the state
/// the field's own `/V` names, falling back to `/Off`. Anything else is the library's own failure
/// rather than a widget the flatten skips: it resolves the appearance of every widget it paints.
fn selected_appearance(
    document: &Document,
    widget: ObjectId,
    field: &Field,
) -> Result<ObjectId, Failure> {
    // The library looks the state up once — the value's, or `/Off` when the value names none — and
    // requires what it finds to be a stream reference, so a state that is not one is a failure rather
    // than a reason to try the other.
    let on_state = with_inherited(document, field.id, b"V", Ascent::Member, |value| {
        value.as_name().ok().map(<[u8]>::to_vec)
    })?
    .flatten();
    let stream = with_normal_appearance(document, widget, |appearance| match appearance {
        Object::Reference(id) => Some(*id),
        Object::Dictionary(states) if matches!(field.kind, Kind::Checkbox | Kind::Radio) => {
            [on_state.as_deref(), Some(b"Off".as_slice())]
                .into_iter()
                .flatten()
                .find_map(|state| states.get(state).ok())
                .and_then(|state| state.as_reference().ok())
        }
        _ => None,
    })
    .flatten();
    stream.ok_or_else(Failure::engine)
}

/// The page a widget sits on: the one its `/P` names, when the document's page list holds it, or
/// else the one whose annotations list it. A `/P` naming anything else falls back to the
/// annotation search, as the reference's does.
fn widget_page(document: &Document, pages: &[ObjectId], widget: ObjectId) -> Option<ObjectId> {
    let named = graph::dictionary(document, widget)
        .and_then(|dictionary| dictionary.get(b"P").ok())
        .and_then(|page| page.as_reference().ok())
        .filter(|page| pages.contains(page));
    if let Some(page) = named {
        return Some(page);
    }
    pages.iter().copied().find(|page| {
        graph::dictionary(document, *page)
            .and_then(|dictionary| dictionary.get(b"Annots").ok())
            .and_then(|annots| page::resolved(document, annots).as_array().ok())
            .is_some_and(|annots| {
                annots
                    .iter()
                    .any(|annot| annot.as_reference().ok() == Some(widget))
            })
    })
}

/// Remove one widget from a page's annotation list.
fn remove_annot(document: &mut Document, page: ObjectId, widget: ObjectId) {
    let Some(dictionary) = graph::dictionary_mut(document, page) else {
        return;
    };
    let Ok(annots) = dictionary.get(b"Annots") else {
        return;
    };
    let Ok(values) = annots.as_array().map(|values| graph::clone_array(values)) else {
        return;
    };
    // The list is rebuilt rather than filtered in place: a `retain` drops what it removes with the
    // language's own recursion, and an entry may be nested as deep as the scan builds one.
    let mut kept = Vec::with_capacity(values.len());
    for annot in values {
        if annot.as_reference().ok() == Some(widget) {
            graph::discard(annot);
        } else {
            kept.push(annot);
        }
    }
    graph::set(dictionary, b"Annots", Object::Array(kept));
}

/// The document's `/AcroForm` object, if the file states it as a reference.
fn acroform_id(document: &Document) -> Option<ObjectId> {
    let root = document.trailer.get(b"Root").ok()?.as_reference().ok()?;
    graph::dictionary(document, root)?
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
    if let Some(dictionary) = graph::dictionary_mut(document, id) {
        graph::set(dictionary, key, value);
    }
}

/// Remove one key from one dictionary.
fn remove_key(document: &mut Document, id: ObjectId, key: &[u8]) {
    if let Some(dictionary) = graph::dictionary_mut(document, id)
        && let Some(removed) = dictionary.remove(key)
    {
        graph::discard(removed);
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
    use lopdf::StringFormat;

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
        graph::set(&mut document.trailer, b"Root", Object::Reference(catalog));
        document
    }

    fn catalogue_of(document: &mut Document) -> String {
        let fields = parse(document, false).expect("the test forms are well formed");
        catalogue(document, &fields).expect("the test forms state no malformed option")
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

    /// The `/AcroForm` id `form_with_fields` names.
    fn acroform_of(document: &Document) -> ObjectId {
        let root = document
            .trailer
            .get(b"Root")
            .expect("the form states a catalog")
            .as_reference()
            .expect("the catalog is a reference");
        graph::dictionary(document, root)
            .and_then(|catalog| catalog.get(b"AcroForm").ok())
            .and_then(|form| form.as_reference().ok())
            .expect("the catalog names the form")
    }

    /// The first field `form_with_fields` stated, as the id its `/Fields` entry names.
    fn first_field(document: &Document, acroform: ObjectId) -> ObjectId {
        graph::dictionary(document, acroform)
            .and_then(|form| form.get(b"Fields").ok())
            .and_then(|fields| fields.as_array().ok())
            .and_then(|fields| fields.first())
            .and_then(|field| field.as_reference().ok())
            .expect("the form states a field")
    }

    fn set_fields(document: &mut Document, acroform: ObjectId, entries: Vec<Object>) {
        let dictionary =
            graph::dictionary_mut(document, acroform).expect("the form is a dictionary");
        graph::set(dictionary, b"Fields", Object::Array(entries));
    }

    /// A named container holding `kid` as its one child, with the entries `states` on its dictionary.
    fn container(
        document: &mut Document,
        name: &str,
        kid: ObjectId,
        states: impl FnOnce(&mut Dictionary),
    ) -> ObjectId {
        let parent = document.add_object(Dictionary::new());
        set_field_entry(document, kid, b"Parent", Object::Reference(parent));
        let mut children = Dictionary::new();
        states(&mut children);
        children.set(
            "T",
            Object::String(name.as_bytes().to_vec(), StringFormat::Literal),
        );
        children.set("Kids", Object::Array(vec![Object::Reference(kid)]));
        let dictionary = graph::dictionary_mut(document, parent).expect("the container is one");
        for (key, value) in &children {
            graph::set(dictionary, key.clone(), graph::clone(value));
        }
        parent
    }

    fn set_field_entry(document: &mut Document, field: ObjectId, key: &[u8], value: Object) {
        let dictionary = graph::dictionary_mut(document, field).expect("the field is a dictionary");
        graph::set(dictionary, key.to_vec(), value);
    }

    #[test]
    fn a_fields_entry_that_names_an_array_is_read() {
        // `PDFAcroForm.Fields` is `dict.lookup('Fields')`, one step: the list may be an object of its
        // own, and the reference fills the fields it holds. Measured, the probe row "a /Fields entry
        // that names the array" produces.
        let mut document = form_with_fields(1, |_| "f0".to_string());
        let acroform = acroform_of(&document);
        let field = first_field(&document, acroform);
        let array = document.add_object(Object::Array(vec![Object::Reference(field)]));
        set_field_entry(&mut document, acroform, b"Fields", Object::Reference(array));
        let fields = parse(&mut document, false).expect("a list one step away is the same list");
        assert_eq!(fields.len(), 1);
        assert_eq!(fields[0].name, "f0");
    }

    #[test]
    fn a_kids_entry_that_names_an_array_is_read() {
        // `isNonTerminalAcroField` and `PDFAcroField.Kids` both read `/Kids` one step, so a container
        // whose kid list is an object of its own is a container and its kids are fields of it.
        let mut document = form_with_fields(1, |_| "Q".to_string());
        let acroform = acroform_of(&document);
        let kid = first_field(&document, acroform);
        let parent = container(&mut document, "P", kid, |_| {});
        let array = document.add_object(Object::Array(vec![Object::Reference(kid)]));
        set_field_entry(&mut document, parent, b"Kids", Object::Reference(array));
        set_fields(&mut document, acroform, vec![Object::Reference(parent)]);
        let fields = parse(&mut document, false).expect("a kid list one step away is a list");
        assert_eq!(fields.len(), 1);
        assert_eq!(fields[0].name, "P.Q");
    }

    #[test]
    fn a_kid_stated_in_place_is_no_field_of_the_container() {
        // `createPDFAcroFields` keeps a kid only when the array states a reference that names a
        // dictionary, so a kid written out where it stands is no field of the reference's: measured, the
        // probe row "a /Kids entry that is a field dictionary in place" lists no fillable fields.
        let mut document = form_with_fields(1, |_| "Q".to_string());
        let acroform = acroform_of(&document);
        let kid = first_field(&document, acroform);
        let parent = container(&mut document, "P", kid, |_| {});
        let mut placed = Dictionary::new();
        placed.set("FT", Object::Name(b"Tx".to_vec()));
        placed.set("T", Object::String(b"Q".to_vec(), StringFormat::Literal));
        placed.set("Parent", Object::Reference(parent));
        set_field_entry(
            &mut document,
            parent,
            b"Kids",
            Object::Array(vec![Object::Dictionary(placed)]),
        );
        set_fields(&mut document, acroform, vec![Object::Reference(parent)]);
        let fields = parse(&mut document, false).expect("a kid in place is well formed");
        assert!(fields.is_empty(), "a kid stated in place is no field");
    }

    #[test]
    fn a_null_type_is_inherited_from_the_parent_that_states_one() {
        // `getInheritableAttribute` skips a stated null — `PDFDict.get` answers undefined for one — so a
        // field whose own `/FT` is null takes the type its parent states. Measured, the probe row
        // "a /FT null under a parent stating /FT" produces.
        let mut document = form_with_fields(1, |_| "f0".to_string());
        let acroform = acroform_of(&document);
        let kid = first_field(&document, acroform);
        set_field_entry(&mut document, kid, b"FT", Object::Null);
        let parent = container(&mut document, "P", kid, |states| {
            states.set("FT", Object::Name(b"Tx".to_vec()));
        });
        set_fields(&mut document, acroform, vec![Object::Reference(parent)]);
        let fields =
            parse(&mut document, false).expect("a null is no type, and the parent states one");
        assert_eq!(fields.len(), 1);
        assert_eq!(fields[0].name, "P.f0");
        assert_eq!(fields[0].kind, Kind::Text);
    }

    #[test]
    fn a_name_stated_as_a_reference_that_names_nothing_is_no_name() {
        // `lookupMaybe` answers undefined for a reference that names nothing, exactly as for an absent
        // entry, so the field is named by the chain's other links rather than faulting. Measured, the
        // probe row "a /T stated as a reference that names nothing" fills `P.undefined`.
        let mut document = form_with_fields(1, |_| "f0".to_string());
        let acroform = acroform_of(&document);
        let kid = first_field(&document, acroform);
        set_field_entry(&mut document, kid, b"T", Object::Reference((99, 0)));
        let parent = container(&mut document, "P", kid, |_| {});
        set_fields(&mut document, acroform, vec![Object::Reference(parent)]);
        let fields = parse(&mut document, false).expect("a name that names nothing is no fault");
        assert_eq!(fields.len(), 1);
        assert_eq!(fields[0].name, "P.undefined");
    }

    #[test]
    fn a_terminal_with_an_empty_kids_array_draws_no_widget() {
        // `getWidgets` answers no widget for an empty `/Kids` array; only a field that states none at
        // all draws itself.
        let mut document = form_with_fields(1, |_| "f0".to_string());
        let acroform = acroform_of(&document);
        let field = first_field(&document, acroform);
        set_field_entry(&mut document, field, b"Kids", Object::Array(Vec::new()));
        let fields = parse(&mut document, false).expect("an empty kid list is well formed");
        assert_eq!(fields.len(), 1);
        assert!(
            fields[0].widgets.is_empty(),
            "an empty list states no widget"
        );
    }

    #[test]
    fn a_kid_that_is_not_a_dictionary_is_the_widget_reads_fault() {
        // The reference reads a terminal's widget list with `lookup(idx, PDFDict)` for every fillable
        // kind, whatever it goes on to do with it: measured, a text field whose `/Kids` states a number
        // faults the fill with `Expected instance of PDFDict`.
        let mut document = form_with_fields(1, |_| "f0".to_string());
        let acroform = acroform_of(&document);
        let field = first_field(&document, acroform);
        set_field_entry(
            &mut document,
            field,
            b"Kids",
            Object::Array(vec![Object::Integer(42)]),
        );
        let fields = parse(&mut document, false).expect("the tree itself is well formed");
        assert!(
            fields[0].widget_fault,
            "a widget that is not a dictionary is the fault the widget read throws"
        );
    }

    #[test]
    fn a_flags_entry_that_names_nothing_is_no_flags() {
        // `Ff()` reads the chain with `lookupMaybe`, which answers undefined for a reference that names
        // nothing: the flags are zero rather than the fault a typed read would raise. Measured, the probe
        // row "a choice field whose /Ff names nothing" produces.
        let mut document = form_with_fields(1, |_| "f0".to_string());
        let acroform = acroform_of(&document);
        let field = first_field(&document, acroform);
        set_field_entry(&mut document, field, b"FT", Object::Name(b"Ch".to_vec()));
        set_field_entry(&mut document, field, b"Ff", Object::Reference((9_999, 0)));
        let fields = parse(&mut document, false).expect("a /Ff that names nothing is no flags");
        assert_eq!(fields[0].kind, Kind::OptionList);
    }

    #[test]
    fn a_negative_real_flags_is_read_as_the_reference_reads_it() {
        // `hasFlag` tests `getFlags() & flag`, and JavaScript's `&` converts its left operand to a
        // signed 32-bit integer: `-1.5` is `-1`, so every bit is set. A choice field is therefore a
        // combo box, a button is a push button — the reference tests that bit before the radio bit —
        // and a text field is multi-line, where a whole-number read made a list box, a radio group and
        // a single line of them.
        let mut document = form_with_fields(1, |_| "f0".to_string());
        let acroform = acroform_of(&document);
        let field = first_field(&document, acroform);
        set_field_entry(&mut document, field, b"Ff", Object::Real(-1.5));
        set_field_entry(&mut document, field, b"FT", Object::Name(b"Ch".to_vec()));
        let fields = parse(&mut document, false).expect("the flags are a number");
        assert_eq!(fields[0].kind, Kind::Dropdown, "the combo bit is set");
        set_field_entry(&mut document, field, b"FT", Object::Name(b"Btn".to_vec()));
        let fields = parse(&mut document, false).expect("the flags are a number");
        assert_eq!(
            fields[0].kind,
            Kind::PushButton,
            "the push-button bit is read first"
        );
        set_field_entry(&mut document, field, b"FT", Object::Name(b"Tx".to_vec()));
        let fields = parse(&mut document, false).expect("the flags are a number");
        let marks = marks_of(&document, &fields[0]).expect("the flags are a number");
        assert!(marks.multiline, "the multiline bit is set too");
    }

    #[test]
    fn a_flags_value_past_a_32_bit_integer_is_read_as_its_low_bits() {
        // The conversion keeps the low 32 bits of the number the reference states. A real past 2^32
        // has no bit the kind or the appearance reads — measured, a choice field whose `/Ff` is `1e20`
        // is a list box there — while an integer is read through the double its digits state first, so
        // 2^53 + 131071 rounds up onto the combo bit.
        let mut document = form_with_fields(1, |_| "f0".to_string());
        let acroform = acroform_of(&document);
        let field = first_field(&document, acroform);
        set_field_entry(&mut document, field, b"FT", Object::Name(b"Ch".to_vec()));
        set_field_entry(&mut document, field, b"Ff", Object::Real(1e20));
        let fields = parse(&mut document, false).expect("the flags are a number");
        assert_eq!(fields[0].kind, Kind::OptionList);
        set_field_entry(
            &mut document,
            field,
            b"Ff",
            Object::Integer(9_007_199_254_872_063),
        );
        let fields = parse(&mut document, false).expect("the flags are a number");
        assert_eq!(fields[0].kind, Kind::Dropdown);
    }

    #[test]
    fn a_flag_set_on_negative_real_flags_is_written_as_the_reference_writes_it() {
        // `setFlag` writes `getFlags() | flag`, a 32-bit result: a field whose `/Ff` is `-1.5` keeps
        // `-1` rather than being rewritten as the one bit the fill turned on.
        let mut document = form_with_fields(1, |_| "f0".to_string());
        let acroform = acroform_of(&document);
        let field = first_field(&document, acroform);
        set_field_entry(&mut document, field, b"Ff", Object::Real(-1.5));
        set_flag(&mut document, field, EDIT_FLAG).expect("the flag is written");
        let stated = graph::dictionary(&document, field)
            .and_then(|dictionary| dictionary.get(b"Ff").ok().cloned());
        match stated {
            Some(Object::Integer(flags)) => assert_eq!(flags, -1),
            other => panic!("the flags are written as an integer of their bits: {other:?}"),
        }
    }

    #[test]
    fn a_parent_stated_in_place_is_followed_for_the_entries_a_field_inherits() {
        // The constructor's `ascend` reads each link with `lookupMaybe(…, PDFDict)`, which follows a
        // dictionary stated in place: a field with no type of its own under a parent that states one is
        // that type. `getParent`, the link a *name* is built through, follows a reference only, so the
        // field is still named by its own `/T`. Measured, the probe row "a field whose /Parent is a
        // dictionary in place" produces.
        let mut document = form_with_fields(1, |_| "Q".to_string());
        let acroform = acroform_of(&document);
        let field = first_field(&document, acroform);
        set_field_entry(&mut document, field, b"FT", Object::Null);
        let mut parent = Dictionary::new();
        parent.set("T", Object::String(b"P".to_vec(), StringFormat::Literal));
        parent.set("FT", Object::Name(b"Tx".to_vec()));
        set_field_entry(&mut document, field, b"Parent", Object::Dictionary(parent));
        let fields = parse(&mut document, false).expect("the parent states the type");
        assert_eq!(fields.len(), 1);
        assert_eq!(fields[0].kind, Kind::Text);
        assert_eq!(fields[0].name, "Q");
    }

    /// A form of one `/Ch` field stating `/Opt [(b) (a)]`, whose `Sort` flag `place` puts where it
    /// belongs: the options its appearance lists, in the order it lists them.
    fn sort_probe(place: impl FnOnce(&mut Document, ObjectId)) -> Vec<String> {
        let mut document = form_with_fields(1, |_| "f0".to_string());
        let acroform = acroform_of(&document);
        let field = first_field(&document, acroform);
        set_field_entry(&mut document, field, b"FT", Object::Name(b"Ch".to_vec()));
        set_field_entry(
            &mut document,
            field,
            b"Opt",
            Object::Array(vec![
                Object::String(b"b".to_vec(), StringFormat::Literal),
                Object::String(b"a".to_vec(), StringFormat::Literal),
            ]),
        );
        place(&mut document, field);
        let fields = parse(&mut document, false).expect("the form is well formed");
        listed_options(&document, &fields[0]).expect("the options are readable")
    }

    #[test]
    fn a_sort_flag_is_read_through_the_parent_link_that_states_it() {
        // `isSorted` reads the `Sort` bit with `getFlags`, whose ascent is `getParent`: a flag on the
        // field or on a parent named by a reference sorts the list, and one on a parent stated in place
        // — which `getParent` does not follow — does not. Measured on the reference's own appearance
        // streams: "a" is drawn above "b" for the first two, "b" above "a" for the third.
        let sort = 1 << 19;
        let flag = || {
            let mut flags = Dictionary::new();
            flags.set("Ff", Object::Integer(sort));
            flags
        };
        assert_eq!(
            sort_probe(|document, field| {
                set_field_entry(document, field, b"Ff", Object::Integer(sort));
            }),
            ["a", "b"],
            "a flag the field states itself sorts its list"
        );
        assert_eq!(
            sort_probe(|document, field| {
                let parent = document.add_object(flag());
                set_field_entry(document, field, b"Parent", Object::Reference(parent));
            }),
            ["a", "b"],
            "a parent a reference names states the flag"
        );
        assert_eq!(
            sort_probe(|document, field| {
                set_field_entry(document, field, b"Parent", Object::Dictionary(flag()));
            }),
            ["b", "a"],
            "a parent stated in place is no parent of the later read"
        );
    }

    #[test]
    fn a_choice_field_takes_its_options_from_its_own_dictionary_only() {
        // `Opt()` is `this.dict.lookupMaybe` with no ascent, so a `/Opt` the field's parent states is
        // none of the field's own. Measured, the reference answers "offers no choice to select" for a
        // field whose only `/Opt` is its parent's, whether that parent is named or stated in place.
        for in_place in [false, true] {
            let mut document = form_with_fields(1, |_| "f0".to_string());
            let acroform = acroform_of(&document);
            let field = first_field(&document, acroform);
            set_field_entry(&mut document, field, b"FT", Object::Name(b"Ch".to_vec()));
            let mut parent = Dictionary::new();
            parent.set(
                "Opt",
                Object::Array(vec![
                    Object::String(b"a".to_vec(), StringFormat::Literal),
                    Object::String(b"b".to_vec(), StringFormat::Literal),
                ]),
            );
            let stated = if in_place {
                Object::Dictionary(parent)
            } else {
                Object::Reference(document.add_object(parent))
            };
            set_field_entry(&mut document, field, b"Parent", stated);
            let fields = parse(&mut document, false).expect("the form is well formed");
            assert_eq!(
                choice_options(&document, &fields[0]).expect("no option is malformed"),
                Vec::<String>::new(),
                "a parent's options are none of the field's (in place: {in_place})"
            );
        }
    }

    #[test]
    fn a_kid_whose_name_is_a_stated_null_is_a_widget() {
        // `PDFDict.has` answers false for a stated null, so a kid that states `/T null` is no field of
        // its parent's: `getWidgets` lists it and the fill writes an appearance into it. Measured, the
        // reference's output carries one.
        let mut document = form_with_fields(1, |_| "f0".to_string());
        let acroform = acroform_of(&document);
        let field = first_field(&document, acroform);
        let mut kid = Dictionary::new();
        kid.set("T", Object::Null);
        let kid = document.add_object(kid);
        set_field_entry(
            &mut document,
            field,
            b"Kids",
            Object::Array(vec![Object::Reference(kid)]),
        );
        let fields = parse(&mut document, false).expect("the form is well formed");
        assert_eq!(fields[0].widgets, vec![kid], "the kid draws an appearance");
        assert!(!fields[0].widget_fault, "a stated null is no fault");
    }

    #[test]
    fn a_name_is_read_along_a_chain_past_the_readings_bound() {
        // The reading half stops its own field walk at `pdf_field_depth_max`; this path terminates on the
        // cycle instead, so a `/Parent` chain longer than that bound is named in full, as
        // `getFullyQualifiedName` walks it — and a chain that closes still ends.
        let length = vocabulary().limits.pdf_field_depth_max + 8;
        let mut document = Document::with_version("1.7");
        let mut deepest = None;
        let mut links = Vec::new();
        for index in 0..length {
            let mut link = Dictionary::new();
            link.set(
                "T",
                Object::String(format!("n{index}").into_bytes(), StringFormat::Literal),
            );
            if let Some(parent) = deepest {
                link.set("Parent", Object::Reference(parent));
            }
            deepest = Some(document.add_object(link));
            links.push(deepest.expect("a link"));
        }
        let name = qualified_name(&document, links[length - 1]).expect("every link states a name");
        assert_eq!(name.split('.').count(), length, "no link is cut off");
        let first = links[0];
        set_field_entry(
            &mut document,
            first,
            b"Parent",
            Object::Reference(links[length - 1]),
        );
        assert_eq!(
            qualified_name(&document, links[length - 1])
                .expect("a chain that closes is still read")
                .split('.')
                .count(),
            length,
            "the walk ends where the chain closes"
        );
    }

    #[test]
    fn a_field_tree_is_walked_past_the_readings_bound_and_stops_at_a_cycle() {
        // A kid chain is walked to any depth, so a tree deeper than the reading half's bound still
        // yields its terminal field; a container whose kid names one of its own ancestors ends there
        // rather than recursing, where the reference's own walk dies with a stack overflow
        // (`PARSER.md` §5.17).
        let depth = vocabulary().limits.pdf_field_depth_max + 8;
        let mut document = Document::with_version("1.7");
        let mut leaf = Dictionary::new();
        leaf.set("FT", Object::Name(b"Tx".to_vec()));
        leaf.set("T", Object::String(b"Q".to_vec(), StringFormat::Literal));
        let mut outer = document.add_object(leaf);
        for _ in 0..depth {
            let mut container = Dictionary::new();
            container.set("T", Object::String(b"P".to_vec(), StringFormat::Literal));
            container.set("Kids", Object::Array(vec![Object::Reference(outer)]));
            let parent = document.add_object(container);
            set_field_entry(&mut document, outer, b"Parent", Object::Reference(parent));
            outer = parent;
        }
        let mut acroform = Dictionary::new();
        acroform.set("Fields", Object::Array(vec![Object::Reference(outer)]));
        let acroform = document.add_object(acroform);
        let mut catalog = Dictionary::new();
        catalog.set("AcroForm", Object::Reference(acroform));
        let catalog = document.add_object(catalog);
        graph::set(&mut document.trailer, b"Root", Object::Reference(catalog));
        let fields = parse(&mut document, false).expect("the tree itself is well formed");
        assert_eq!(fields.len(), 1, "the terminal field is reached");
        assert_eq!(fields[0].kind, Kind::Text);

        // Two containers that name each other's `Kids`: the walk stops where the chain closes.
        let mut document = form_with_fields(1, |_| "f0".to_string());
        let acroform = acroform_of(&document);
        let first = document.add_object(Dictionary::new());
        let second = document.add_object(Dictionary::new());
        for (this, other) in [(first, second), (second, first)] {
            set_field_entry(
                &mut document,
                this,
                b"T",
                Object::String(b"P".to_vec(), StringFormat::Literal),
            );
            set_field_entry(
                &mut document,
                this,
                b"Kids",
                Object::Array(vec![Object::Reference(other)]),
            );
        }
        set_fields(&mut document, acroform, vec![Object::Reference(first)]);
        let fields = parse(&mut document, false).expect("a cycle is no fault");
        assert!(
            fields.is_empty(),
            "a cycle yields no field and does not recurse"
        );
    }

    #[test]
    fn a_kid_a_container_names_twice_is_a_field_of_it_twice() {
        // A kid the same container names twice is no cycle: the field is a kid of that container twice
        // and is visited twice, as the reference's own walk visits it.
        let mut document = form_with_fields(1, |_| "Q".to_string());
        let acroform = acroform_of(&document);
        let field = first_field(&document, acroform);
        let twice = container(&mut document, "P", field, |_| {});
        set_fields(&mut document, acroform, vec![Object::Reference(twice)]);
        set_field_entry(
            &mut document,
            twice,
            b"Kids",
            Object::Array(vec![Object::Reference(field), Object::Reference(field)]),
        );
        let fields = parse(&mut document, false).expect("a duplicated kid is no fault");
        assert_eq!(fields.len(), 2, "the kid is a field of its container twice");
        assert_eq!(fields[0].name, "P.Q");
        assert_eq!(fields[1].name, "P.Q");
    }

    #[test]
    fn a_field_whose_kids_is_no_array_is_the_widget_reads_fault() {
        // `Kids()` is a typed `lookupMaybe`, so a `/Kids` that is a number throws where the list is
        // read, while the container test above it reads the same entry untyped and sees no kids.
        let mut document = form_with_fields(1, |_| "f0".to_string());
        let acroform = acroform_of(&document);
        let field = first_field(&document, acroform);
        set_field_entry(&mut document, field, b"Kids", Object::Integer(42));
        let fields = parse(&mut document, false).expect("the tree itself is well formed");
        assert!(fields[0].widget_fault);
        assert_eq!(fields[0].widgets, Vec::new());
    }

    #[test]
    fn a_signature_reads_its_kids_only_to_flatten() {
        // A signature's appearance is never regenerated, so the reference reads its widget list only
        // when the form is flattened: the same form produces without `flatten` and faults with it.
        let mut document = form_with_fields(1, |_| "f0".to_string());
        let acroform = acroform_of(&document);
        let field = first_field(&document, acroform);
        set_field_entry(&mut document, field, b"FT", Object::Name(b"Sig".to_vec()));
        set_field_entry(
            &mut document,
            field,
            b"Kids",
            Object::Array(vec![Object::Integer(42)]),
        );
        assert!(
            !parse(&mut document, false).expect("a fill reads no signature's kids")[0].widget_fault,
            "a signature's widget list is not read for a fill"
        );
        assert!(
            parse(&mut document, true).expect("the tree is well formed")[0].widget_fault,
            "flattening reads it, and the read is the fault"
        );
    }

    #[test]
    fn a_fields_entry_that_names_a_reference_is_the_references_fault() {
        let mut document = form_with_fields(1, |_| "f0".to_string());
        let acroform = acroform_of(&document);
        let field = first_field(&document, acroform);
        // The entry names an object that is a reference itself: the reference's typed lookup takes
        // one step, finds a value that is not a dictionary, and throws.
        let link = document.add_object(Object::Reference(field));
        set_fields(&mut document, acroform, vec![Object::Reference(link)]);
        let Err(failure) = parse(&mut document, false) else {
            panic!("a field entry that names a reference is a fault");
        };
        assert!(
            !failure.is_usage(),
            "the reference faults on the form rather than refusing the request"
        );
    }

    #[test]
    fn a_parent_link_that_names_a_reference_is_the_references_fault() {
        let mut document = form_with_fields(1, |_| "f0".to_string());
        let acroform = acroform_of(&document);
        let field = first_field(&document, acroform);
        let parent = document.add_object(Object::Reference(field));
        let dictionary =
            graph::dictionary_mut(&mut document, field).expect("the field is a dictionary");
        graph::set(dictionary, b"Parent", Object::Reference(parent));
        let Err(failure) = parse(&mut document, false) else {
            panic!("a parent link that names a reference is a fault");
        };
        assert!(
            !failure.is_usage(),
            "the reference faults on the form rather than refusing the request"
        );
    }

    /// Point a field's `/key` at a reference that names a reference to `target`: the value the
    /// reference's typed lookup takes one step to is a reference, which is the fault it throws.
    fn chain_entry(document: &mut Document, field: ObjectId, key: &[u8], target: ObjectId) {
        let link = document.add_object(Object::Reference(target));
        let dictionary = graph::dictionary_mut(document, field).expect("the field is a dictionary");
        graph::set(dictionary, key, Object::Reference(link));
    }

    #[test]
    fn a_field_name_that_names_a_reference_is_the_references_fault() {
        // `/T` is read with a typed lookup that takes one step and demands a string, so a name stated
        // as a reference to a reference faults the fill rather than leaving the field unnamed and
        // silently unfilled.
        let mut document = form_with_fields(1, |_| "f0".to_string());
        let acroform = acroform_of(&document);
        let field = first_field(&document, acroform);
        let name = document.add_object(Object::String(b"f0".to_vec(), StringFormat::Literal));
        chain_entry(&mut document, field, b"T", name);
        let Err(failure) = parse(&mut document, false) else {
            panic!("a name that names a reference is a fault");
        };
        assert!(
            !failure.is_usage(),
            "the reference faults on the form rather than refusing the request"
        );
    }

    #[test]
    fn a_field_type_that_names_a_reference_is_the_references_fault() {
        // `/FT` is read with a typed lookup that demands a name, so a type stated as a reference to a
        // reference faults rather than leaving the fill to read no type at all.
        let mut document = form_with_fields(1, |_| "f0".to_string());
        let acroform = acroform_of(&document);
        let field = first_field(&document, acroform);
        let kind = document.add_object(Object::Name(b"Tx".to_vec()));
        chain_entry(&mut document, field, b"FT", kind);
        let Err(failure) = parse(&mut document, false) else {
            panic!("a type that names a reference is a fault");
        };
        assert!(
            !failure.is_usage(),
            "the reference faults on the form rather than refusing the request"
        );
    }

    #[test]
    fn a_field_flags_entry_that_names_a_reference_is_the_references_fault() {
        // `/Ff` decides what a `/Ch` or a `/Btn` field is, and the reference reads it with a typed
        // lookup that demands a number: flags stated as a reference to a reference fault there rather
        // than being read as no flags. A text field's flags are read only while its appearance is laid
        // out, which `a_text_field_whose_flags_name_a_reference_faults_by_its_appearance` states.
        let mut document = form_with_fields(1, |_| "f0".to_string());
        let acroform = acroform_of(&document);
        let field = first_field(&document, acroform);
        let dictionary =
            graph::dictionary_mut(&mut document, field).expect("the field is a dictionary");
        graph::set(dictionary, b"FT", Object::Name(b"Btn".to_vec()));
        let flags = document.add_object(Object::Integer(0));
        chain_entry(&mut document, field, b"Ff", flags);
        let Err(failure) = parse(&mut document, false) else {
            panic!("flags that name a reference are a fault");
        };
        assert!(
            !failure.is_usage(),
            "the reference faults on the form rather than refusing the request"
        );
    }

    #[test]
    fn a_text_field_whose_flags_name_a_reference_faults_by_its_appearance() {
        // A `/Tx` field of no flags is a tree the reference builds and a form it fills; the flags are
        // read where the field's appearance is laid out, and that read is the fault. Measured: a form
        // whose unfilled text field states such flags produces, and one the request fills faults.
        let mut document = form_with_fields(1, |_| "f0".to_string());
        let acroform = acroform_of(&document);
        let field = first_field(&document, acroform);
        let flags = document.add_object(Object::Integer(0));
        chain_entry(&mut document, field, b"Ff", flags);
        let fields = parse(&mut document, false).expect("a text field's flags are not read here");
        assert_eq!(fields.len(), 1);
        assert!(
            marks_of(&document, &fields[0]).is_err(),
            "the appearance's own read is the fault"
        );
    }

    #[test]
    fn a_field_that_states_no_type_is_the_references_fault() {
        // `/FT` is the one entry of a field whose typed lookup has no absent case: the reference reads
        // it to decide what kind of field it has, so a terminal field that states no type anywhere on
        // its chain is a form it faults on, whatever value the request asks for.
        let mut document = form_with_fields(1, |_| "f0".to_string());
        let acroform = acroform_of(&document);
        let field = first_field(&document, acroform);
        let dictionary =
            graph::dictionary_mut(&mut document, field).expect("the field is a dictionary");
        dictionary.remove(b"FT");
        let Err(failure) = parse(&mut document, false) else {
            panic!("a field of no type is a fault");
        };
        assert!(
            !failure.is_usage(),
            "the reference faults on the form rather than refusing the request"
        );
    }

    #[test]
    fn a_field_name_stated_as_a_name_is_read_as_its_text() {
        // A `/T` stated as a name is no string to the reference, whose typed lookup throws where it
        // reads that name to match a field; this engine reads the name as the text it states, so the
        // field is addressable here. `PARSER.md` §5.16 states the difference.
        let mut document = form_with_fields(1, |_| "f0".to_string());
        let acroform = acroform_of(&document);
        let field = first_field(&document, acroform);
        let dictionary =
            graph::dictionary_mut(&mut document, field).expect("the field is a dictionary");
        graph::set(dictionary, b"T", Object::Name(b"f0".to_vec()));
        let fields = parse(&mut document, false).expect("a name is a name the engine reads");
        assert_eq!(fields.len(), 1);
        assert_eq!(fields[0].name, "f0");
    }

    #[test]
    fn a_parent_link_that_names_nothing_is_the_references_fault() {
        // The name is built through `PDFAcroField.getParent`, a typed `lookup` that throws on a link
        // that names nothing — so the reference faults as soon as it reads a name, which it does for
        // every field while it updates the appearances of the form.
        let mut document = form_with_fields(1, |_| "f0".to_string());
        let acroform = acroform_of(&document);
        let field = first_field(&document, acroform);
        let dictionary =
            graph::dictionary_mut(&mut document, field).expect("the field is a dictionary");
        graph::set(dictionary, b"Parent", Object::Reference((9_999, 0)));
        let Err(failure) = parse(&mut document, false) else {
            panic!("a link that names nothing is the fault the name lookup throws");
        };
        assert!(
            !failure.is_usage(),
            "the library faults rather than refusing"
        );
    }

    #[test]
    fn a_name_comes_from_the_parent_chain_and_an_absent_link_states_undefined() {
        // `getFullyQualifiedName` joins the chain `/Parent` states, not the tree the walk reaches
        // through `/Kids`: a named child whose `/Parent` is absent is named by itself, one that states
        // no name under a named parent is named `P.undefined`, and one whose parent states no name is
        // named `undefined.Q` — each the name a request must ask for.
        let mut document = Document::with_version("1.7");
        let mut parent = Dictionary::new();
        parent.set("T", Object::String(b"P".to_vec(), StringFormat::Literal));
        let parent = document.add_object(parent);
        let unnamed_parent = Dictionary::new();
        let unnamed_parent = document.add_object(unnamed_parent);
        let terminal = |document: &mut Document, name: Option<&[u8]>, parent: Option<ObjectId>| {
            let mut field = Dictionary::new();
            field.set("FT", Object::Name(b"Tx".to_vec()));
            if let Some(name) = name {
                field.set("T", Object::String(name.to_vec(), StringFormat::Literal));
            }
            if let Some(parent) = parent {
                field.set("Parent", Object::Reference(parent));
            }
            document.add_object(field)
        };
        let named = terminal(&mut document, Some(b"Q"), Some(parent));
        let nameless = terminal(&mut document, None, Some(parent));
        let orphan = terminal(&mut document, Some(b"R"), None);
        let under_unnamed = terminal(&mut document, Some(b"S"), Some(unnamed_parent));
        for (parent, kids) in [
            (parent, vec![named, nameless]),
            (unnamed_parent, vec![under_unnamed]),
        ] {
            graph::set(
                graph::dictionary_mut(&mut document, parent).expect("a dictionary"),
                b"Kids",
                Object::Array(kids.into_iter().map(Object::Reference).collect()),
            );
        }
        let mut acroform = Dictionary::new();
        acroform.set(
            "Fields",
            Object::Array(
                [parent, unnamed_parent, orphan]
                    .into_iter()
                    .map(Object::Reference)
                    .collect(),
            ),
        );
        let acroform = document.add_object(acroform);
        let mut catalog = Dictionary::new();
        catalog.set("AcroForm", Object::Reference(acroform));
        let catalog = document.add_object(catalog);
        graph::set(&mut document.trailer, b"Root", Object::Reference(catalog));
        let fields = parse(&mut document, false).expect("the field tree is well formed");
        assert_eq!(
            fields
                .iter()
                .map(|field| field.name.as_str())
                .collect::<Vec<_>>(),
            vec!["P.Q", "P.undefined", "undefined.S", "R"],
            "the chain, not the tree, states the name"
        );
    }

    #[test]
    fn a_field_of_a_type_the_reference_knows_no_field_of_is_not_a_field() {
        // `createPDFAcroTerminal` builds a `PDFAcroTerminal` for a type it does not know, and the form
        // converts that to no field at all: it is neither listed nor addressable, where a field of a
        // known type that cannot take a value is refused as one. So the engine drops it here rather
        // than answering a sentence of its own about it — without reading its `/Ff`, which the
        // reference reads only for the types whose kind that entry decides.
        let mut document = form_with_fields(1, |_| "f0".to_string());
        let acroform = acroform_of(&document);
        let field = first_field(&document, acroform);
        let dictionary =
            graph::dictionary_mut(&mut document, field).expect("the field is a dictionary");
        graph::set(dictionary, b"FT", Object::Name(b"Foo".to_vec()));
        let flags = document.add_object(Object::Integer(0));
        chain_entry(&mut document, field, b"Ff", flags);
        let fields = parse(&mut document, false).expect("a type of no known field is not a fault");
        assert!(fields.is_empty(), "no field is built from it");
    }

    #[test]
    fn a_signature_never_reads_its_flags() {
        // `createPDFAcroTerminal` reads `/Ff` for a `/Ch` and a `/Btn` only, and a signature's
        // appearance is never regenerated: a `/Sig` field whose flags are a reference to a reference
        // is no fault to the reference, and is none here either.
        let mut document = form_with_fields(1, |_| "f0".to_string());
        let acroform = acroform_of(&document);
        let field = first_field(&document, acroform);
        let dictionary =
            graph::dictionary_mut(&mut document, field).expect("the field is a dictionary");
        graph::set(dictionary, b"FT", Object::Name(b"Sig".to_vec()));
        let flags = document.add_object(Object::Integer(0));
        chain_entry(&mut document, field, b"Ff", flags);
        let fields = parse(&mut document, false).expect("a signature states no flags to read");
        assert_eq!(fields.len(), 1);
        assert_eq!(fields[0].kind, Kind::Signature);
    }

    #[test]
    fn a_fields_dictionary_in_place_and_a_field_with_no_parent_are_still_read() {
        let mut document = Document::with_version("1.7");
        let mut field = Dictionary::new();
        field.set("FT", Object::Name(b"Tx".to_vec()));
        field.set("T", Object::String(b"f0".to_vec(), StringFormat::Literal));
        let mut acroform = Dictionary::new();
        acroform.set("Fields", Object::Array(vec![Object::Dictionary(field)]));
        let acroform = document.add_object(acroform);
        let mut catalog = Dictionary::new();
        catalog.set("AcroForm", Object::Reference(acroform));
        let catalog = document.add_object(catalog);
        graph::set(&mut document.trailer, b"Root", Object::Reference(catalog));
        let fields = parse(&mut document, false)
            .expect("a dictionary in place and no parent are well formed");
        assert_eq!(fields.len(), 1);
        assert_eq!(fields[0].name, "f0");
        assert_eq!(fields[0].kind, Kind::Text, "the field is a text field");
    }

    #[test]
    fn a_fields_entry_that_is_not_an_array_fills_nothing_and_is_left_as_stated() {
        // The reference's `normalizedEntries` writes an empty array over an entry that is not one; this
        // engine leaves the entry as the file states it. Neither fills a field from it, so no answer
        // differs — only the entry the produced file carries, which the canonical model does not read.
        let mut document = Document::with_version("1.7");
        let mut acroform = Dictionary::new();
        acroform.set("Fields", Object::Name(b"None".to_vec()));
        let acroform = document.add_object(acroform);
        let mut catalog = Dictionary::new();
        catalog.set("AcroForm", Object::Reference(acroform));
        let catalog = document.add_object(catalog);
        graph::set(&mut document.trailer, b"Root", Object::Reference(catalog));
        let fields =
            parse(&mut document, false).expect("a form whose list is not a list is well formed");
        assert!(fields.is_empty(), "no field can be read from it");
        assert_eq!(
            graph::dictionary(&document, acroform)
                .and_then(|form| form.get(b"Fields").ok())
                .and_then(|fields| fields.as_name().ok().map(<[u8]>::to_vec)),
            Some(b"None".to_vec()),
            "the entry is left as the file states it"
        );
    }
}
