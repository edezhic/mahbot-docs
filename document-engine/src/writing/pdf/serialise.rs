//! A document as the bytes it is written as.
//!
//! The layout is the object model's own, entry for entry, so a document written here and one
//! written by [`lopdf::Document::save_to`] are the same bytes — a test pins that. Three things it
//! does differently, all because a file may state them and the object model cannot write them:
//!
//! - An object the scan could not parse is written as the bytes it stated, verbatim, which is what
//!   the reference's writer does with such an object. Its dictionary is empty and its bytes are the
//!   object's own, marked by the position the scan recorded.
//! - The index is sparse: one section per run of consecutive object numbers the document actually
//!   holds, rather than one entry per number up to the highest. A file may number an object in the
//!   hundreds of millions, and the reference writes one back; an index that walked every number up
//!   to it could not.
//! - Every object is written, including one whose dictionary states a container's type. The object
//!   model leaves those out because it writes the containers itself; here the scan has already
//!   expanded or dropped every real one, so what is left is a plain dictionary the reference writes
//!   too.
//!
//! Every walk here keeps its own stack, so an object nested as deep as the scan builds one costs
//! heap rather than the process's stack.

use std::io::Write as _;

use lopdf::{Dictionary, Object, StringFormat};

use super::graph;

/// One index section: the first number it states, how many entries it holds, and the entries
/// themselves — a generation and the offset the object begins at.
type Section = (u32, u32, Vec<(u16, usize)>);

/// The document as the bytes to write: the header, every object, the index and the trailer.
pub(super) fn document(document: &lopdf::Document) -> Vec<u8> {
    let mut out = Vec::new();
    let _ = writeln!(out, "%PDF-{}", document.version);
    // The binary mark tells a tool the file holds 8-bit binary. The object model states one that is
    // not a valid mark only for a document it read from a file that had none, and such a file is
    // written without one.
    if document.binary_mark.iter().all(|&byte| byte & 0x80 != 0) {
        out.push(b'%');
        out.extend_from_slice(&document.binary_mark);
        out.push(b'\n');
    }
    // One entry per object written, in the order the object model states them. Number zero is the
    // free-list head every index begins with, so an index cannot state an object there: the scan has
    // already left out every object numbered zero — the one at generation zero is the head itself and
    // is not an object, one at a generation the model holds is reported as an object the index cannot
    // name (`PARSER.md` §1.7), and one at the marker generation was given a number of its own before
    // it reached here — and the guard below is what keeps a document built by this engine from stating
    // one. One number is one entry, whatever generations the file states for it: the index has no room
    // for a second, so the last body written is the one it names — the shape the scan reports as the
    // object the index cannot name (§1.6). Nothing else is left out. The scan expands an object stream
    // and never assigns an index stream, so neither reaches here; a plain dictionary that merely states
    // such a type is a dictionary like any other, and the reference writes it.
    let mut entries: Vec<(u32, u16, usize)> = Vec::new();
    for (&(number, generation), object) in &document.objects {
        if number == 0 {
            continue;
        }
        match entries.last_mut() {
            Some(last) if last.0 == number => *last = (number, generation, out.len()),
            _ => entries.push((number, generation, out.len())),
        }
        if let Some(bytes) = graph::carried(object) {
            let _ = writeln!(out, "{number} {generation} obj");
            out.extend_from_slice(bytes);
            out.extend_from_slice(b"\nendobj\n");
            continue;
        }
        let _ = write!(
            out,
            "{number} {generation} obj\n{}",
            if separator(object) { " " } else { "" }
        );
        write_object(&mut out, object);
        let _ = writeln!(
            out,
            "{}\nendobj",
            if end_separator(object) { " " } else { "" }
        );
    }
    let index = out.len();
    // One section per run of consecutive numbers the document holds. The free entry at zero counts
    // towards the first section's length, as the object model states it.
    let mut sections: Vec<Section> = vec![(0, 1, Vec::new())];
    for &(number, generation, offset) in &entries {
        let last = sections.last_mut().expect("a section");
        if number == last.0 + last.1 {
            last.1 += 1;
            last.2.push((generation, offset));
        } else {
            sections.push((number, 1, vec![(generation, offset)]));
        }
    }
    out.extend_from_slice(b"xref\n");
    for (start, count, held) in &sections {
        let _ = writeln!(out, "{start} {count}");
        if *start == 0 {
            out.extend_from_slice(b"0000000000 65535 f \n");
        }
        for &(generation, offset) in held {
            let _ = writeln!(out, "{offset:>010} {generation:>05} n ");
        }
    }
    let mut trailer = graph::clone_dictionary(&document.trailer);
    graph::set(&mut trailer, "Size", i64::from(document.max_id) + 1);
    out.extend_from_slice(b"trailer\n");
    let root = Object::Dictionary(trailer);
    write_object(&mut out, &root);
    graph::discard(root);
    let _ = write!(out, "\nstartxref\n{index}\n%%EOF");
    out
}

/// Whether a value needs a space before it, as the object model's own writer states it.
fn separator(object: &Object) -> bool {
    matches!(
        object,
        Object::Null
            | Object::Boolean(_)
            | Object::Integer(_)
            | Object::Real(_)
            | Object::Reference(_)
    )
}

/// Whether a value needs a space after it.
fn end_separator(object: &Object) -> bool {
    matches!(
        object,
        Object::Null
            | Object::Boolean(_)
            | Object::Integer(_)
            | Object::Real(_)
            | Object::Name(_)
            | Object::Reference(_)
            | Object::Stream(_)
    )
}

/// One object's bytes, written from its own syntax without recursing.
fn write_object(out: &mut Vec<u8>, object: &Object) {
    /// One thing to write: a value still to be taken apart, a dictionary to state, a name to escape, or
    /// bytes already decided.
    enum Step<'a> {
        Value(&'a Object),
        Dictionary(&'a Dictionary),
        Name(&'a [u8]),
        Bytes(&'a [u8]),
    }
    let mut stack = vec![Step::Value(object)];
    while let Some(step) = stack.pop() {
        match step {
            Step::Bytes(bytes) => out.extend_from_slice(bytes),
            Step::Name(name) => write_name(out, name),
            Step::Value(Object::Null) => out.extend_from_slice(b"null"),
            Step::Value(Object::Boolean(true)) => out.extend_from_slice(b"true"),
            Step::Value(Object::Boolean(false)) => out.extend_from_slice(b"false"),
            Step::Value(Object::Integer(value)) => {
                let _ = write!(out, "{value}");
            }
            Step::Value(Object::Real(value)) => {
                let _ = write!(out, "{value}");
            }
            Step::Value(Object::Name(name)) => write_name(out, name),
            Step::Value(Object::String(text, format)) => write_string(out, text, *format),
            Step::Value(Object::Reference((number, generation))) => {
                let _ = write!(out, "{number} {generation} R");
            }
            Step::Value(Object::Array(values)) => {
                stack.push(Step::Bytes(b"]"));
                for (index, value) in values.iter().enumerate().rev() {
                    stack.push(Step::Value(value));
                    if index > 0 && separator(value) {
                        stack.push(Step::Bytes(b" "));
                    }
                }
                stack.push(Step::Bytes(b"["));
            }
            Step::Value(Object::Dictionary(dictionary)) | Step::Dictionary(dictionary) => {
                stack.push(Step::Bytes(b">>"));
                for (key, value) in dictionary.iter().rev() {
                    stack.push(Step::Value(value));
                    if separator(value) {
                        stack.push(Step::Bytes(b" "));
                    }
                    stack.push(Step::Name(key));
                }
                stack.push(Step::Bytes(b"<<"));
            }
            Step::Value(Object::Stream(stream)) => {
                stack.push(Step::Bytes(b"\nendstream"));
                stack.push(Step::Bytes(&stream.content));
                stack.push(Step::Bytes(b"stream\n"));
                stack.push(Step::Dictionary(&stream.dict));
            }
        }
    }
}

/// A name's bytes as the format spells them: white space, a delimiter and a byte that is not printable
/// are written as `#` and two upper-case hexadecimal digits.
fn write_name(out: &mut Vec<u8>, name: &[u8]) {
    out.push(b'/');
    for &byte in name {
        if b" \t\n\r\x0C()<>[]{}/%#".contains(&byte) || !(33..=126).contains(&byte) {
            let _ = write!(out, "#{byte:02X}");
        } else {
            out.push(byte);
        }
    }
}

/// A string as the format spells it: a literal one with its parentheses and backslashes escaped, or a
/// hexadecimal one.
fn write_string(out: &mut Vec<u8>, text: &[u8], format: StringFormat) {
    match format {
        StringFormat::Literal => {
            // An unbalanced `)` and a `(` are escaped, and so is a backslash and a carriage return,
            // which a reader would otherwise read as a line break.
            let mut escaped = vec![false; text.len()];
            let mut open: Vec<usize> = Vec::new();
            for (index, &byte) in text.iter().enumerate() {
                match byte {
                    b'(' => open.push(index),
                    b')' => match open.pop() {
                        Some(_) => {}
                        None => escaped[index] = true,
                    },
                    b'\\' | b'\r' => escaped[index] = true,
                    _ => {}
                }
            }
            for index in open {
                escaped[index] = true;
            }
            out.push(b'(');
            for (index, &byte) in text.iter().enumerate() {
                if escaped[index] {
                    out.push(b'\\');
                    out.push(if byte == b'\r' { b'r' } else { byte });
                } else {
                    out.push(byte);
                }
            }
            out.push(b')');
        }
        StringFormat::Hexadecimal => {
            out.push(b'<');
            for &byte in text {
                let _ = write!(out, "{byte:02X}");
            }
            out.push(b'>');
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lopdf::{Object, Stream};

    /// A document holding every object shape the format states, so the layout is compared on all of
    /// them: a name whose bytes must be escaped, both string formats, every scalar, a nested array and
    /// dictionary, a stream inside a dictionary, and a stream as an object of its own — the shape every
    /// content stream takes, whose separator the writer states differently.
    fn every_shape() -> lopdf::Document {
        let mut document = lopdf::Document::with_version("1.7");
        let mut dictionary = Dictionary::new();
        dictionary.set("Type", Object::Name(b"Catalog".to_vec()));
        dictionary.set("A#20B", Object::Name(b"A B".to_vec()));
        dictionary.set(
            "Literal",
            Object::String(b"(a\\b) \r".to_vec(), StringFormat::Literal),
        );
        dictionary.set(
            "Hex",
            Object::String(vec![0, 1, 0xFE], StringFormat::Hexadecimal),
        );
        dictionary.set("Real", Object::Real(0.5));
        dictionary.set("Integer", Object::Integer(-7));
        dictionary.set("True", Object::Boolean(true));
        dictionary.set("False", Object::Boolean(false));
        dictionary.set("Null", Object::Null);
        dictionary.set(
            "Nested",
            Object::Array(vec![
                Object::Integer(1),
                Object::Array(vec![Object::Name(b"x".to_vec()), Object::Null]),
                Object::Reference((2, 0)),
            ]),
        );
        document.add_object(dictionary);
        document.add_object(Object::Dictionary(Dictionary::from_iter([(
            "Stream".to_string(),
            Object::Stream(Stream::new(
                Dictionary::from_iter([("Length".to_string(), Object::Integer(3))]),
                b"abc".to_vec(),
            )),
        )])));
        // A stream stated as an object of its own, which is what a content stream is.
        document.add_object(Stream::new(
            Dictionary::from_iter([("Length".to_string(), Object::Integer(3))]),
            b"xyz".to_vec(),
        ));
        document.trailer.set("Root", Object::Reference((1, 0)));
        document
    }

    #[test]
    fn the_layout_is_the_object_models_own() {
        // The bytes a document is written as are the ones the object model writes, entry for entry: the
        // layout is reproduced rather than invented, so no produced file is changed by it.
        let mut theirs = Vec::new();
        let mut document = every_shape();
        document.reference_table.cross_reference_type = lopdf::xref::XrefType::CrossReferenceTable;
        document
            .save_to(&mut theirs)
            .expect("the object model writes it");
        assert_eq!(super::document(&every_shape()), theirs);
    }

    #[test]
    fn an_object_numbered_zero_is_not_written() {
        // Number zero is the free-list head every index begins with, so an index cannot state an
        // object there. The reference writes the object anyway and its index then names the object
        // after it, which is not a file a reader follows; this engine leaves it out, and the scan
        // reports the object the produced file does not hold (`PARSER.md` §1.7).
        let mut document = every_shape();
        document.objects.insert((0, 5), Object::Integer(7));
        let bytes = super::document(&document);
        let text = String::from_utf8_lossy(&bytes);
        assert!(!text.contains("0 5 obj"), "{text}");
        assert_eq!(text.matches("\nxref\n").count(), 1, "{text}");
        assert_eq!(text.matches("0000000000 65535 f \n").count(), 1, "{text}");
    }

    #[test]
    fn a_dictionary_that_states_a_containers_type_is_written() {
        // The object model's own writer leaves out every object typed `/ObjStm`, `/XRef` or
        // `/Linearized`, because it writes those containers itself. This writer never receives one: the
        // scan expands an object stream and never assigns an index stream. What it can receive is a
        // plain dictionary that merely states such a type, and leaving that out would state a reference
        // to an object the file no longer holds — so it is written, as the reference writes it.
        let mut document = every_shape();
        let (id, _) = document.add_object(Object::Dictionary(Dictionary::from_iter([(
            "Type".to_string(),
            Object::Name(b"ObjStm".to_vec()),
        )])));
        let bytes = super::document(&document);
        let header = format!("\n{id} 0 obj\n").into_bytes();
        let offset = bytes
            .windows(header.len())
            .position(|window| window == header)
            .expect("the object is written")
            + 1;
        let text = String::from_utf8_lossy(&bytes).into_owned();
        assert!(
            text.contains(&format!("{offset:>010} 00000 n ")),
            "the index states where it begins at {offset}: {text}"
        );
    }

    #[test]
    fn a_number_stated_at_two_generations_is_one_index_entry() {
        // A file may state one number at two generations. Both bodies are written, and the index names
        // the last one — the body a reader reaches — because an index has one entry per number, and two
        // sections beginning at the same number is not an index.
        let mut document = every_shape();
        let number = document.max_id + 1;
        document.add_object(Object::Integer(1));
        document.objects.insert((number, 1), Object::Integer(2));
        let bytes = super::document(&document);
        let text = String::from_utf8_lossy(&bytes).into_owned();
        assert!(text.contains(&format!("\n{number} 0 obj\n")), "{text}");
        assert!(text.contains(&format!("\n{number} 1 obj\n")), "{text}");
        let header = format!("\n{number} 1 obj\n").into_bytes();
        let offset = bytes
            .windows(header.len())
            .position(|window| window == header)
            .expect("the second body is written")
            + 1;
        assert!(
            text.contains(&format!("{offset:>010} 00001 n ")),
            "the index names the last body: {text}"
        );
        assert!(
            !text.contains(&format!("\n{number} 1\n")),
            "no section begins at the number: {text}"
        );
    }

    #[test]
    fn a_carried_object_is_written_as_the_bytes_it_stated() {
        // An object the scan could not parse is written verbatim, between its header and its `endobj`,
        // and the index still states where it begins.
        let mut document = every_shape();
        let mut carried = Stream::new(Dictionary::new(), b"<< /Broken !!".to_vec());
        carried.start_position = Some(0);
        let (id, _) = document.add_object(Object::Stream(carried));
        let bytes = super::document(&document);
        let text = String::from_utf8_lossy(&bytes);
        assert!(
            text.contains(&format!("{id} 0 obj\n<< /Broken !!"))
                && text.contains("<< /Broken !!\nendobj"),
            "{text}"
        );
    }
}
