//! The object graph a page action builds on: the document, the walk that takes it apart, and the copy
//! that rebuilds it.
//!
//! One rule holds this module together: **no value a file states is ever cloned or dropped with the
//! language's own recursion.** A value read from a body may be nested tens of thousands of levels deep,
//! the language's clone and drop cost one stack frame per level, and a stack overflow is not a panic
//! the engine's containment can catch — it aborts the process. So every clone, every replacement and
//! every drop here goes through [`clone`], [`set`], [`set_object`](Document::set_object) and
//! [`discard`], which keep their own stack, and a call site that clones or sets a value it read from a
//! document with the object model's own methods is a defect.
//!
//! Two things guard that rule, and neither is complete on its own. `tests/pdf_dictionary_writes.rs`
//! scans the family for the object model's own `Dictionary::set` and for a write into the object map
//! outside this file and `scan.rs`, so those two writes cannot be spelled elsewhere. What a source scan
//! cannot see is a clone or a drop that happens by going out of scope — a `Vec<Object>`, an
//! `Option<Object>`, a `Stream` — so each path that touches a value a file states has a case in
//! `writing::pdf::tests` on the harness's two-mebibyte stack. A walk added without one is a defect the
//! suite cannot see, which is why the rule is stated here rather than left to the scan.
//!
//! The scan's [`object_depth_max`](crate::vocabulary::EngineBounds::object_depth_max) does not carry
//! this: it bounds the memory a document may cost, and it sits where the reference's own parser gives
//! up. A bound low enough for the language's recursion to survive would carry a page or a page tree
//! nested past it, and a carried object the page tree needs refuses a file the reference reads — which
//! is the outcome this whole change exists to remove.

use std::collections::{BTreeMap, HashMap, HashSet};

use lopdf::{Dictionary, Object, ObjectId, Stream};

/// A document whose object graph is dismantled iteratively when it is dropped. Everything else about it
/// is the object model's own, reached through [`Deref`](std::ops::Deref).
pub(crate) struct Document {
    pub(super) inner: lopdf::Document,
    /// Every object number a reference the input states names. A number handed to a new object is one
    /// this set does not hold, so a reference the input leaves dangling keeps naming nothing and a
    /// reference it states keeps naming what it named.
    pub(super) named: HashSet<u32>,
    /// Where the search for a free number resumes: every number below it has been looked at.
    pub(super) spare: u32,
}

impl std::ops::Deref for Document {
    type Target = lopdf::Document;

    fn deref(&self) -> &lopdf::Document {
        &self.inner
    }
}

impl std::ops::DerefMut for Document {
    fn deref_mut(&mut self) -> &mut lopdf::Document {
        &mut self.inner
    }
}

impl Drop for Document {
    fn drop(&mut self) {
        // The trailer may be nested as deep as the file states, and the language's own drop of such a
        // value recurses once per level.
        let mut pending: Vec<Object> = std::mem::take(&mut self.inner.objects)
            .into_values()
            .collect();
        pending.push(Object::Dictionary(std::mem::take(&mut self.inner.trailer)));
        discard_all(pending);
    }
}

/// Drop `object` without recursing: a value read from a body may be nested as deep as the scan builds
/// one, and the language's own drop of such a value costs one stack frame per level. A scalar holds
/// nothing, so it is dropped here without a worklist — this is the family's hottest write path.
pub(super) fn discard(object: Object) {
    if !matches!(
        object,
        Object::Array(_) | Object::Dictionary(_) | Object::Stream(_)
    ) {
        return;
    }
    discard_all([object]);
}

/// Drop every object in `pending`, and everything each of them holds, from the same worklist. Whatever a
/// walk has not reached yet — the rest of a `Vec`, or the rest of an array's iterator — is a call site.
pub(super) fn discard_all(pending: impl IntoIterator<Item = Object>) {
    let mut pending: Vec<Object> = pending.into_iter().collect();
    while let Some(object) = pending.pop() {
        match object {
            Object::Array(values) => pending.extend(values),
            Object::Dictionary(dictionary) => {
                pending.extend(dictionary.into_iter().map(|(_, value)| value));
            }
            Object::Stream(stream) => {
                pending.extend(stream.dict.into_iter().map(|(_, value)| value));
            }
            _ => {}
        }
    }
}

impl Document {
    /// An empty document stating `version`.
    pub(super) fn with_version(version: &str) -> Self {
        Self {
            inner: lopdf::Document::with_version(version),
            named: HashSet::new(),
            spare: 1,
        }
    }

    /// Add `object` under a new number. The number is free, so nothing is replaced.
    pub(super) fn add_object<T: Into<Object>>(&mut self, object: T) -> ObjectId {
        let id = (self.next_number(), 0);
        self.set_object(id, object);
        id
    }

    /// Store `object` at `id`, dismantling what was there. The object model's own `set_object` drops
    /// the value it replaces with the language's recursion, and that value may be nested as deep as the
    /// scan builds one.
    pub(super) fn set_object<T: Into<Object>>(&mut self, id: ObjectId, object: T) {
        if let Some(replaced) = self.inner.objects.insert(id, object.into()) {
            discard(replaced);
        }
    }

    /// A number for a new object, with nothing stored at it: what a copy reserves before it writes.
    /// Nothing is written at a reserved number, so a reference the source does not hold stays dangling
    /// rather than resolving to an object this engine invented.
    pub(super) fn new_object_id(&mut self) -> ObjectId {
        (self.next_number(), 0)
    }

    /// The next number: the one past the highest the document holds and that no reference the input
    /// states names. A number a dangling reference names would make that reference resolve, and one an
    /// object already holds at another generation would leave the index naming one body per number; both
    /// are skipped. The search resumes past the last number handed out, so a run of them costs the
    /// numbers they take rather than their square.
    fn next_number(&mut self) -> u32 {
        let number = free_number(
            &self.inner.objects,
            &self.named,
            self.inner.max_id,
            &mut self.spare,
        );
        self.inner.max_id = self.inner.max_id.max(number);
        number
    }
}

/// A number no object in `objects` states at any generation and no reference in `named` names: the one
/// past `highest` while the format has room, otherwise the lowest such number. `cursor` is where the
/// search resumes — every number below it was taken or named when it was passed, and neither set
/// shrinks — so a run of reservations costs the numbers they take rather than their square.
///
/// The search looks at most one candidate more than the two sets hold, where a free number cannot
/// fail to be: so it is total, and file-derived data never has to end in a panic. A document that
/// held every number would need billions of objects, which no file states.
pub(super) fn free_number(
    objects: &BTreeMap<ObjectId, Object>,
    named: &HashSet<u32>,
    highest: u32,
    cursor: &mut u32,
) -> u32 {
    // The index states one entry per number, so a number any generation holds is taken.
    let free = |number: u32| {
        !named.contains(&number)
            && objects
                .range((number, 0)..=(number, u16::MAX))
                .next()
                .is_none()
    };
    // While the format has room the answer is the number past every one the file states or names;
    // past its ceiling the search resumes at the cursor.
    let start = if highest == u32::MAX {
        (*cursor).max(1)
    } else {
        highest + 1
    };
    let candidates = named.len().saturating_add(objects.len()).saturating_add(1);
    let number = (start..=u32::MAX)
        .chain(1..start)
        .take(candidates)
        .find(|number| free(*number))
        .unwrap_or(start);
    *cursor = if number == u32::MAX { 1 } else { number + 1 };
    number
}

/// One step of the reference's `lookup`: the object a reference names, or the value itself. `None`
/// when the reference names nothing. A reference that names a further reference is *that reference*,
/// which is what `lookup` answers, so every read of a file-stated entry stops here — in the page
/// walk, in the form's field tree and in a stream's own resources alike. A value reached through two
/// links is a value neither side reads, and a typed read that finds a reference where it wanted a
/// dictionary, an array or a number is the library's own fault.
pub(super) fn deref<'a>(
    objects: &'a BTreeMap<ObjectId, Object>,
    object: &'a Object,
) -> Option<&'a Object> {
    match object {
        Object::Reference(id) => objects.get(id),
        other => Some(other),
    }
}

/// [`deref`] for a write: the object a reference names, or the value itself, mutably. `None` when
/// the reference names nothing. A reference that names a further reference is that reference, as in
/// [`deref`], so a write through it reaches the further reference and not the value past it.
pub(super) fn deref_mut<'a>(
    objects: &'a mut BTreeMap<ObjectId, Object>,
    object: &'a mut Object,
) -> Option<&'a mut Object> {
    match object {
        Object::Reference(id) => objects.get_mut(id),
        other => Some(other),
    }
}

/// [`deref`] for a read that must answer a value: a reference that names nothing answers
/// [`Object::Null`], which every typed read then refuses — the reference's own `lookup` answers
/// `undefined` there, and its typed reads throw on it.
pub(super) fn resolved<'a>(
    objects: &'a BTreeMap<ObjectId, Object>,
    object: &'a Object,
) -> &'a Object {
    deref(objects, object).unwrap_or(&Object::Null)
}

/// The dictionary a file-stated reference names, in one step. `None` when it names nothing, when it
/// names a value that is not a dictionary, or when it names a further reference — a typed lookup that
/// finds a reference where it wanted a dictionary is a fault in the reference's library, so such an
/// object is a write the engine declines rather than one it follows.
pub(super) fn dictionary(document: &lopdf::Document, id: ObjectId) -> Option<&Dictionary> {
    document.objects.get(&id)?.as_dict().ok()
}

/// [`dictionary`] for a write.
pub(super) fn dictionary_mut(
    document: &mut lopdf::Document,
    id: ObjectId,
) -> Option<&mut Dictionary> {
    document.objects.get_mut(&id)?.as_dict_mut().ok()
}

/// The bytes an object the scan could not parse stated, or `None` for an object it read. The scan
/// marks such an object with the position its bytes began at; nothing else in a document read from a
/// body carries one. This is the marker's one statement: the scan, the renamer and the writer all
/// read it here.
pub(super) fn carried(object: &Object) -> Option<&[u8]> {
    let stream = object.as_stream().ok()?;
    stream
        .start_position
        .is_some()
        .then_some(stream.content.as_slice())
}

/// Whether `dictionary` is a page leaf, whose `/Parent` a copy never carries: following it would drag
/// the source's page tree, and every page it holds, into the produced file. The reference deletes it
/// from a copied page for the same reason.
pub(super) fn is_page(dictionary: &Dictionary) -> bool {
    dictionary.get_type().is_ok_and(|kind| kind == b"Page")
}

/// A copy of `object`, built on the heap.
pub(super) fn clone(object: &Object) -> Object {
    rebuild(object, &mut |leaf| Some(leaf.clone()), false)
        .expect("a copy that refuses nothing always answers")
}

/// A copy of `dictionary`, built on the heap.
pub(super) fn clone_dictionary(dictionary: &Dictionary) -> Dictionary {
    let mut cloned = Dictionary::new();
    for (key, value) in dictionary {
        cloned.set(key.clone(), clone(value));
    }
    cloned
}

/// A copy of `values`, each element built on the heap.
pub(super) fn clone_array(values: &[Object]) -> Vec<Object> {
    values.iter().map(clone).collect()
}

/// Set `key` on `dictionary`, taking whatever it stated out of the way without the language's own
/// recursive drop: the value replaced may be nested as deep as the scan builds one. A key the
/// dictionary already states keeps its place, as the object model's own `set` leaves it, so a produced
/// file states its entries in the order the input did.
///
/// Every write into a dictionary in this family goes through here rather than the object model's own
/// `set`, which the module rule forbids outside this file — a fresh dictionary and one read from a body
/// look the same at a call site, and only one of them is safe to replace into. A test scans the family
/// for the forbidden call.
pub(super) fn set<K, V>(dictionary: &mut Dictionary, key: K, value: V)
where
    K: Into<Vec<u8>>,
    V: Into<Object>,
{
    let key = key.into();
    let value = value.into();
    match dictionary.get_mut(key.as_slice()) {
        Ok(slot) => discard(std::mem::replace(slot, value)),
        Err(_) => dictionary.set(key, value),
    }
}

/// A copy of `object` with every reference `renames` names stated under the id it names instead. A
/// reference the map does not name is carried as it stands.
pub(super) fn rename(object: &Object, renames: &HashMap<ObjectId, ObjectId>) -> Object {
    rebuild(
        object,
        &mut |leaf| {
            Some(match leaf {
                Object::Reference(id) => Object::Reference(renames.get(id).copied().unwrap_or(*id)),
                other => other.clone(),
            })
        },
        false,
    )
    .expect("a copy that refuses nothing always answers")
}

/// One object with every reference it holds replaced by the id its copy was reserved, and — for a page
/// leaf — its `/Parent` dropped. `None` when a reference names no reserved copy, which is a defect in
/// what the caller reserved rather than a dangling id to write.
pub(super) fn rewrite(object: &Object, memo: &HashMap<ObjectId, ObjectId>) -> Option<Object> {
    rebuild(
        object,
        &mut |leaf| match leaf {
            Object::Reference(id) => Some(Object::Reference(*memo.get(id)?)),
            other => Some(other.clone()),
        },
        true,
    )
}

/// A container being rebuilt: the source's entries, how many are done, and the container built so
/// far.
enum Frame<'a> {
    Array(&'a [Object], usize, Vec<Object>),
    Dictionary(Vec<(&'a [u8], &'a Object)>, usize, Dictionary),
    Stream(&'a Stream, Vec<(&'a [u8], &'a Object)>, usize, Dictionary),
}

/// `object` rebuilt from its own syntax: every array, dictionary and stream taken apart and put back
/// together on `stack`, so the depth costs heap rather than the process's stack. `leaf` answers what a
/// value that is not a container becomes, and `parent` drops a page leaf's `/Parent`.
fn rebuild(
    object: &Object,
    leaf: &mut dyn FnMut(&Object) -> Option<Object>,
    parent: bool,
) -> Option<Object> {
    fn entries(dictionary: &Dictionary, parent: bool) -> Vec<(&[u8], &Object)> {
        let skip = parent && is_page(dictionary);
        dictionary
            .iter()
            .filter(|(key, _)| !(skip && key.as_slice() == b"Parent"))
            .map(|(key, value)| (key.as_slice(), value))
            .collect()
    }

    let mut stack: Vec<Frame<'_>> = Vec::new();
    let mut built: Option<Object> = None;
    let mut descend = true;
    loop {
        if descend {
            descend = false;
            let current: &Object = match stack.last() {
                None => object,
                Some(Frame::Array(values, index, _)) => &values[*index],
                Some(
                    Frame::Dictionary(entries, index, _) | Frame::Stream(_, entries, index, _),
                ) => entries[*index].1,
            };
            let opened = match current {
                Object::Array(values) if !values.is_empty() => {
                    Some(Frame::Array(values, 0, Vec::with_capacity(values.len())))
                }
                Object::Dictionary(dictionary) => {
                    let entries = entries(dictionary, parent);
                    if entries.is_empty() {
                        built = Some(Object::Dictionary(Dictionary::new()));
                        None
                    } else {
                        Some(Frame::Dictionary(entries, 0, Dictionary::new()))
                    }
                }
                Object::Stream(stream) => {
                    let entries = entries(&stream.dict, parent);
                    if entries.is_empty() {
                        built = Some(rebuilt_stream(stream, Dictionary::new()));
                        None
                    } else {
                        Some(Frame::Stream(stream, entries, 0, Dictionary::new()))
                    }
                }
                other => {
                    let Some(value) = leaf(other) else {
                        discard_frames(&mut stack);
                        return None;
                    };
                    built = Some(value);
                    None
                }
            };
            if let Some(frame) = opened {
                stack.push(frame);
                descend = true;
                continue;
            }
        }
        let Some(value) = built.take() else {
            // Unreachable: every branch above either opens a frame or states a value. What the walk
            // built is dismantled here rather than by the language's own drop, which would recurse.
            discard_frames(&mut stack);
            return None;
        };
        let done = match stack.last_mut() {
            None => return Some(value),
            Some(Frame::Array(values, index, out)) => {
                out.push(value);
                *index += 1;
                *index == values.len()
            }
            Some(
                Frame::Dictionary(entries, index, out) | Frame::Stream(_, entries, index, out),
            ) => {
                out.set(entries[*index].0.to_vec(), value);
                *index += 1;
                *index == entries.len()
            }
        };
        if !done {
            descend = true;
            continue;
        }
        built = Some(match stack.pop().expect("a frame was just read") {
            Frame::Array(_, _, out) => Object::Array(out),
            Frame::Dictionary(_, _, out) => Object::Dictionary(out),
            Frame::Stream(stream, _, _, out) => rebuilt_stream(stream, out),
        });
    }
}

/// `stream` with `dict` in place of its dictionary: the bytes, and the two flags the object model
/// states outside the dictionary, are the ones the source holds.
fn rebuilt_stream(stream: &Stream, dict: Dictionary) -> Object {
    let mut rebuilt = Stream::new(dict, stream.content.clone());
    rebuilt.allows_compression = stream.allows_compression;
    rebuilt.start_position = stream.start_position;
    Object::Stream(rebuilt)
}

/// Drop what a walk gave up on, from one worklist rather than one stack frame per level.
fn discard_frames(stack: &mut Vec<Frame<'_>>) {
    let mut partial: Vec<Object> = Vec::new();
    for frame in stack.drain(..) {
        match frame {
            Frame::Array(_, _, values) => partial.push(Object::Array(values)),
            Frame::Dictionary(_, _, dictionary) | Frame::Stream(_, _, _, dictionary) => {
                partial.push(Object::Dictionary(dictionary));
            }
        }
    }
    discard_all(partial);
}

/// Every reference `object` holds, in the order its own syntax states them.
pub(super) fn references(object: &Object, out: &mut Vec<ObjectId>) {
    let mut pending = vec![object];
    drain(&mut pending, out);
}

/// Whether `object` states any reference `renames` names. Every reference counts, a page leaf's
/// `/Parent` included, because a rewrite renames what a copy leaves alone.
pub(super) fn names_any(object: &Object, renames: &HashMap<ObjectId, ObjectId>) -> bool {
    let mut pending = vec![object];
    while let Some(object) = pending.pop() {
        match object {
            Object::Reference(id) if renames.contains_key(id) => return true,
            Object::Array(values) => pending.extend(values.iter()),
            Object::Dictionary(dictionary) => {
                pending.extend(dictionary.iter().map(|(_, value)| value));
            }
            Object::Stream(stream) => {
                pending.extend(stream.dict.iter().map(|(_, value)| value));
            }
            _ => {}
        }
    }
    false
}

/// Every reference `dictionary` holds, in the order its own syntax states them.
pub(super) fn references_of(dictionary: &Dictionary, out: &mut Vec<ObjectId>) {
    let mut pending = Vec::new();
    push_references(dictionary, &mut pending);
    drain(&mut pending, out);
}

/// Take every reference in `pending` apart into `out`, and everything each of them holds.
fn drain(pending: &mut Vec<&Object>, out: &mut Vec<ObjectId>) {
    while let Some(object) = pending.pop() {
        match object {
            Object::Reference(id) => out.push(*id),
            Object::Array(values) => pending.extend(values.iter().rev()),
            Object::Dictionary(dictionary) => push_references(dictionary, pending),
            Object::Stream(stream) => push_references(&stream.dict, pending),
            _ => {}
        }
    }
}

/// The values a dictionary states, reversed so a stack pops them in the order the file states them, and
/// without the `/Parent` of a page leaf — the one entry a copy never follows.
fn push_references<'a>(dictionary: &'a Dictionary, pending: &mut Vec<&'a Object>) {
    let skip = is_page(dictionary);
    pending.extend(
        dictionary
            .iter()
            .rev()
            .filter(|(key, _)| !(skip && key.as_slice() == b"Parent"))
            .map(|(_, value)| value),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_replaced_key_keeps_its_place() {
        // The object model's own `set` leaves a key where the dictionary already states it, and so does
        // this one: a produced file states its entries in the order the input did.
        let mut dictionary = Dictionary::new();
        dictionary.set("A", Object::Integer(1));
        dictionary.set("B", Object::Integer(2));
        set(&mut dictionary, b"A", Object::Integer(3));
        let keys: Vec<&[u8]> = dictionary.iter().map(|(key, _)| key.as_slice()).collect();
        assert_eq!(keys, vec![b"A".as_slice(), b"B".as_slice()]);
        assert_eq!(dictionary.get(b"A").and_then(Object::as_i64).ok(), Some(3));
    }
}
