# The reference's parser, tolerance by tolerance

The reference reads a PDF with pdf-lib 1.17.1. `PDFParser.parseDocument` walks the body from the
header, `PDFObjectParser` reads one object's syntax, `PDFObjectStreamParser` and
`PDFXRefStreamParser` read the two container kinds, `decodePDFRawStream` decodes a container's
filters, `PDFPageTree.traverse` walks the page tree, and `PDFContext.lookup` is the one dereference
every reader goes through. Its tolerance is not incidental: it is written in those files. This
document enumerates it — every place the reference accepts something a stricter reader refuses, or
recovers where a stricter reader gives up — so that the list, rather than a search run one shape at a
time, is the evidence that this class is closed.

Each entry states the shape, what the reference does with it, what this engine does, and which of
two decisions it takes:

- **matched** — the engine answers the same way, and a corpus case or a crate test witnesses it; where
  the shape changes no answer, the entry says so instead of naming a witness, because none could tell
  the two apart;
- **stated** — the engine answers differently, the reason is given here and beside the operation, and
  where the difference is visible to the harness a case shows it.

A shape that is not named here — or in the closing section, which names the shapes that are not entries
— is one nobody has read the reference's parser for; the files above are where to look. An entry is
added when the shape is found, beside the code that answers it.

`writing/pdf/page.rs` states the rules it enforces and points here for the enumeration rather than
restating it. The entries are numbered by section so a code comment can cite one (`PARSER.md` §3.5).

Where a shape is carried, the engine writes back the bytes the object states, which is what the
reference does; `page.rs` states that rule and `graph.rs` states how such bytes are moved without
recursion. Where an object the file held is *not* in the produced file, or a reference inside a carried
body no longer names what the input named, the answer says so in its loss field — see §1.5–§1.7,
§1.10 and §3.5 — because a produced file that quietly lost what its input held claims a success it did
not have.

## The entries whose answers differ

Every entry says whether this engine answers as the reference does. These are the ones a case or a
reader can tell apart, each marked **stated** where it appears above, with its reason:

- §1.4 — an object header whose numbers the model cannot key: the produced file carries different
  numbers.
- §1.5 — one object number past a `u64` stated in two runs of digits that the reference reads as one
  number: the second body replaces the first, and the answer reports the object the produced file
  does not hold as an object.
- §1.10 — an object this engine places, and a reference inside a body it carried: the object is
  renumbered, so the reference no longer names what the input named.
- §2.4 — the depth band: a page the reference keeps as bytes is turned here.
- §2.6 — a reference whose halves are not whole numbers: the body stating it is carried here.
- §3.5, §3.10, and the loss counts in §1.5–§1.7 — the loss field: the answer says which objects
  the produced file does not hold as an object, how many a container it could not read states, and
  which references no longer name what the input named.
- §4.3 — a `/Root` whose `/Type` is a reference to the name `/Catalog`: this engine adopts the body's
  catalog where the reference keeps a dictionary it cannot use, and its load faults.
- §4.4, §5.1 — a catalog or a page tree stated in place: this engine gives it a number, so the
  produced file is a document where the reference's own output of the same input is not.
- §4.5 — an encrypted file whose body is damaged: this engine names the password first.
- §5.8 — a `/Count` that is not a number: this engine produces where the reference faults.
- §5.9 — a `/Parent` path that returns to where it began: this engine produces where the reference
  faults.
- §5.16 — the field tree's permissive and eager shapes: a `/T` stated as a name is read as its text
  here, where the reference's typed lookup throws once it reaches that field to compare names; on a
  field the request never reaches, a `/T` that is not a string and a signature's `/Parent` link that
  names nothing fault here where the reference fills; a `/Fields` entry that is not an array at all
  is left as the file states it, where the reference writes an empty array in its place; a `/Ff`
  real past what this model's 32-bit float states exactly rounds onto another bit here, where the
  reference reads the decimal as a double; and a `/MaxLen` stated as a real is no bound here, where
  the reference compares the text against it and faults. Three produced entries also differ where no
  reader of the canonical model looks: an in-place field or widget dictionary is named by the
  reference the engine registered; a `/Fields` or `/Kids` entry that names an array is written as the
  array itself; and a `/Ff` real the model's float rounded is written back as the float's spelling
  where the fill leaves it as the file stated — the same rounding no reader of the model sees.
- §5.17 — the field tree's cycle stop: a kid list or a parent link that returns to itself is answered
  here — a refusal that names no field, or a field named by the links the cycle reaches — where the
  reference dies with a stack overflow.
- §6.3 — a whole number past `i64` in an object's body: the object is carried here.

## 1. The parse loop

**1.1 Bytes before the header.** `parseHeader` scans forward for `%PDF-` and reads the version after
it, so a file with a mail header, a byte-order mark or a partial line before the marker is still a
file. The engine's `Scan::header` scans the same way. **Matched**; witnessed by
`...::a_file_with_bytes_before_its_header_is_read`. No case: a file with leading bytes answers exactly
as the same file without them, so the harness cannot tell the two apart.

**1.2 Junk between objects.** `skipJibberish` steps over anything that is not an object header, an
`xref`, a `trailer` or a `startxref`, so a body with a stray line between two objects reads.
`Scan::skip_jibberish` does the same. **Matched**; witnessed by
`...::a_stray_line_between_two_objects_is_skipped`. No case: a stray line between two objects changes
no answer, so the harness cannot tell a walk that skips it from one that does not.

**1.3 A missing `endobj`.** `parseIndirectObject` calls `matchKeyword('endobj')` and ignores the
result. So does the walk. **Matched**; witnessed by `...::an_object_without_an_endobj_is_read`. No
case: a file without the marker answers exactly as the same file with it, so the harness cannot tell
the two apart.

**1.4 An object header whose numbers the model cannot key.** `parseIndirectObjectHeader` reads the
number and the generation with `parseRawInt` and builds `PDFRef.of(number, generation)`, which holds
both as JavaScript numbers with no ceiling, and the writer states that reference as it stands: a
produced file carries `5000000000 0 obj` and an index section starting there, and an object stated at
generation 65536 is written back at that generation. The object model here keys `(u32, u16)`, so an
object stated past either half is held by the number the file states, keyed the way the reference keys
it (§1.5), until the whole body has been read and is then given a number of this document's own, at
generation zero, with every reference the walk parsed following it there. Two numbers the reference
tells apart stay two objects — the map is keyed by the number it reads, so neither merges with the
other. The number given is one past every number the file states or names at any generation — or, when
the file states or names the format's ceiling, the lowest number neither states nor names, as §1.10
states — so a reference the file leaves dangling keeps naming nothing. Every reference the walk parsed
is rewritten to follow the number its object was given (`graph::rename`); a reference inside a body the
walk carried is not rewritten — that body's bytes are written verbatim — and the answer reports it
(§1.10).

An object the file states at the highest generation is held the same way, though the model could key
it: while the body is read, every object the model cannot key is referenced by a *marker*, an id
stating that generation, and a file's own object there would leave one id meaning two objects. It is
written at a number of the document's own, which loses nothing a reader can see — the file's number
was not one this model could keep anyway. A reference the file states at a marker's number is a pair
the map answers, so it names the file's object or nothing, never another pair's marker: a number the
file only names has no object at it, and that reference is written back at the id the file stated — the
number and the generation it used — so what the file left dangling stays where the file left it. A
reference whose number *or generation* is past what the model keys has no such id, so it is written at a
marker's number instead; it names nothing either way.

**Stated**: the numbers the produced file carries differ from the reference's, because this model
cannot key the stated ones. The canonical model normalises object numbers, so the harness cannot see
the difference; what a reader sees — an object, reachable by the references that named it — is the
same. Witnessed by `pdf/pdf_rotate/generation_out_of_range` (a page stated at generation 65536,
turned), `...::an_object_stated_at_a_generation_past_the_format_is_given_a_number_of_its_own`,
`...::an_object_number_past_the_format_is_given_one_of_its_own`,
`...::two_objects_the_model_cannot_key_are_given_two_numbers`,
`...::an_object_the_file_states_at_a_markers_number_is_carried`,
`...::a_reference_the_file_states_at_a_markers_number_stays_dangling`,
`...::a_dangling_reference_keeps_the_number_the_file_stated`,
`...::a_dangling_reference_past_the_model_is_written_at_a_marker`,
`...::a_reference_read_before_its_object_is_given_a_number_follows_it`,
`...::a_reference_the_file_does_not_hold_stays_dangling`,
`...::a_pages_parent_that_is_past_the_model_follows_the_tree` and
`...::a_container_numbers_its_objects_with_the_document_they_join`.

**1.5 A run of digits longer than a `u64`.** `parseRawInt` builds a string and converts it with
`Number`, so the number it answers is a double: a thirty-digit run is a number to the reference, and two
runs that round to the same double are one object number to it while two that do not are two.
`Scan::raw_int` saturates rather than failing, which is what lets §1.4 hold, and `double` reads the same
run the way the reference reads it, so a file it keeps two objects of keeps two here. The generation is
read the same way, because `parseRawInt` answers the reference a double there too: `Scan::stated` keys
both of a pair's runs by the double each states, so two generations it keeps apart are two objects here
(`...::two_generations_past_a_u64_that_the_reference_keys_apart_are_two_objects`) and two it reads as
one are one (`...::two_generations_that_round_to_one_double_are_one_object`). **Matched**;
witnessed by `...::a_run_of_digits_longer_than_a_u64_saturates_rather_than_failing` and
`...::two_numbers_past_a_u64_that_the_reference_keys_apart_are_two_objects` (the runs of 2^64 and 2^65
are two doubles, so both bodies are kept), and by
`pdf_tolerance.rs::two_numbers_past_the_model_that_the_reference_keys_apart_are_both_kept` and
`pdf_damaged_shapes.rs::two_numbers_the_reference_keys_apart_are_kept_by_both_sides`. A container's
entries are keyed the same way — `...::a_containers_entries_are_keyed_by_the_double_their_digits_state`,
`pdf_tolerance.rs::a_container_naming_two_numbers_the_reference_keys_apart_keeps_both` and
`...::a_container_naming_one_number_in_two_digit_runs_loses_one`.

One shape inside it is **stated**: one number stated twice in two *different* runs that round to one
double. The reference keys the two runs as one number, so its context holds the body the later statement
gives and the earlier one is gone from its output — measured, a rotate of a file stating 2^64 and then
2^64+1 writes one body. This engine keys them as one number too, and reports the object the produced
file does not hold in the loss field of §3.5 rather than answering as though it had kept everything;
witnessed by `...::two_numbers_longer_than_a_u64_are_reported_as_the_object_they_lose` and,
for the body the walk carried rather than parsed,
`...::a_number_past_a_u64_stated_twice_with_a_carried_body_loses_the_object_before_it`,
`pdf_tolerance.rs::a_carried_body_that_replaces_a_number_is_reported` and
`pdf_damaged_shapes.rs::a_carried_body_replacing_a_number_is_written_by_both_sides`. Each header that
states a different number replaces the object before it, so `A, B, A` loses two — pinned by
`...::a_number_stated_again_after_another_loses_the_object_between_them` — while the *same* number
twice is a redefinition and loses nothing — however its digits are spelled, which
`...::the_same_number_longer_than_a_u64_stated_twice_loses_nothing`,
`...::the_same_number_spelled_with_a_leading_zero_is_a_redefinition` and
`...::a_containers_entry_and_a_header_stating_one_pair_lose_nothing` pin — and two runs at
*different* generations are two keys, which
`...::two_numbers_past_a_u64_at_different_generations_are_two_objects` pins.

The count is taken where the replacement happens: `Scan::assign`, when the object is kept, whether its
body was parsed or carried. A header that states a *container* the walk expands replaces nothing —
`parseIndirectObject` hands the stream to the container's parser and assigns no object for it, and the
walk does the same — which `pdf_tolerance.rs::a_saturated_container_header_loses_nothing` and
`...::a_container_after_a_saturated_object_loses_nothing` pin, whichever of the two comes first; one the
walk cannot expand is kept as the bytes it states, as the reference keeps it, so it does replace what
the number held, and that is counted.

**1.6 An object stated twice.** `PDFContext.assign` overwrites, so the last definition in the body is
the one the document holds. `Scan::assign` does the same. **Matched**; witnessed by
`pdf/pdf_rotate/last_definition` and
`...::a_body_read_later_is_the_body_the_file_states_last`. One shape inside it is **stated**: the index
states one entry per number, so two bodies at one number in different generations are two objects the
index cannot both name. The last one written is the one it names and the other is unreachable, so the
answer reports it in the loss field of §3.5 —
`pdf_tolerance.rs::two_generations_of_one_number_are_reported_as_the_body_the_index_cannot_name`
and `pdf_damaged_shapes.rs::two_generations_of_one_number_are_reported_by_the_crate`.

**1.7 Object `0 0`.** `parseDocument` deletes it after the walk, because nothing can reference it.
`scan::document` removes it too, and reports nothing: it is the free list's head rather than an object,
so leaving it out loses nothing. **Matched**; witnessed by
`...::an_object_at_the_reserved_number_is_dropped`. No case: the object is unreachable either way, so
no answer names it. An object stated at number zero at a generation the model holds is not
that object, and the index states no entry at number zero at all, so the produced file cannot name it:
this engine leaves it out and reports it in the loss field of §3.5, where the reference writes it where
the file stated it and writes an index that names neither it nor the object after it. One stated at
number zero at a generation *past* what the model keys — §1.4's marker generation included — is held
like any such number and given a number of this document's own, so nothing is lost there.
**Stated** on the first shape; witnessed by
`pdf_tolerance.rs::an_object_at_number_zero_is_left_out_and_reported`,
`pdf_damaged_shapes.rs::an_object_at_number_zero_is_dropped_by_the_crate_and_kept_by_the_reference` and —
for the generation past the model, where both sides keep the object and only its number differs —
`pdf_tolerance.rs::an_object_at_number_zero_at_the_marker_generation_is_renumbered`. No case for the
first shape: the reference's produced file carries an object the model ignores, so the answer differs
only in the crate's loss field.

**1.8 A binary mark after the header.** `skipBinaryHeaderComment` steps one byte past whatever
follows the version line unless it is an object header. The walk does the same. **Matched**;
witnessed by `...::a_binary_mark_after_the_header_is_stepped_over`. No case: the mark is four bytes no
operation can observe, so no answer differs with or without it.

**1.9 A comment between tokens.** `skipWhitespaceAndComments` skips `%` to the end of the line
wherever it meets one. The walk skips them in the same places. **Matched**; witnessed by
`...::a_comment_between_two_tokens_is_skipped`. No case: a comment changes no answer.

**1.10 The number a placed object is given, and the carried reference that loses it.** An object this
engine adds — an inline catalog or page tree given a number (§4.4, §5.1), a copy's reserved object, a
container's object joined to a document — takes `graph::free_number`: the first number past every
number the document holds and every number a reference the file states names, at any generation; or,
when that past-the-ceiling number does not exist, the lowest such number. What the search starts from
is the highest number the caller holds — the document's own for a copy, every number the walk has
taken or been told for the scan — and what it steps over is stated above: a number any generation
holds and a number any reference names. The search resumes past the last number handed out, so a run
of placements costs the numbers they take rather than their square: measured, 12 800 placements are
instant where the quadratic search took about twenty-eight seconds —
`pdf_tolerance.rs::placing_many_objects_past_the_model_costs_the_objects_they_are`
and the probe row "3 200 objects past the model beside a reference naming 4294967295". A number any
reference names — including one found inside a body the walk carried as bytes, by a scan that steps
over that body's comments, literal and hexadecimal strings and names, so a number one of them holds is
not mistaken for a reference — is never handed out, so a dangling reference keeps naming nothing:
`pdf_tolerance.rs::a_new_object_does_not_take_a_number_a_dangling_reference_names` and
`::a_placed_object_does_not_take_a_number_a_carried_reference_names`. That scan is a keyword reading
and not a parse: three runs of digits that lie together in a carried body are one reference to it
whether or not the file meant one, so the numbers it answers are that reading's — a body of arbitrary
text can keep a number free that nothing named.

A carried body's bytes are written back verbatim, so a reference inside one cannot follow its object to
the number that object was given. The reference keeps the object at the stated number, compressed into
an object stream, and its own file does not parse with hayro; this engine renumbers the object and
reports the reference with `pdf_page_references_lost` — the references of the same keyword reading
above, so a carried body that only looks like it holds a reference can be reported as one. **Stated**:
a reader can tell the two files apart, and the produced file says what it could not carry. Witnessed by
`pdf_tolerance.rs::a_reference_inside_a_carried_body_is_reported_when_its_object_moves`,
`...::a_carried_body_at_the_reserved_number_reports_no_broken_reference` — a body the produced file
does not state has no reference to break — and
`pdf_damaged_shapes.rs::a_carried_body_reference_is_reported_by_the_crate_and_kept_by_the_reference`;
`coverage.rs` records the mark unreached, because a corpus case would compare the crate's note against
the reference's silence. `pdf_merge` and `pdf_split` report no reference at all: each builds a document
of its own and numbers every object it copies from one, so what a reference inside a copied carried body
names there is that new numbering and not the input's — the count the scan keeps is about the input, and
neither answer says anything about it. The objects such a call does not carry are still reported.

## 2. An object's body

**2.1 A body that does not parse.** `parseIndirectObjects` catches the failure and calls
`tryToParseInvalidIndirectObject`, which keeps the bytes between the header and `endobj` as a
`PDFInvalidObject`; the writer copies them out uncompressed. The walk keeps them the same way and
`serialise` writes them verbatim. **Matched**; witnessed by `pdf/pdf_rotate/body_unparsable` (the
turn, so a no-op copy cannot pass), `pdf/pdf_merge/body_unparsable`, `pdf/pdf_rotate/page_unparsable`
and `pdf/pdf_merge/page_unparsable` (the unparsable object *is* the page, which the walk passes over),
and by `parity/tests/pdf_damaged_shapes.rs`, which compares the bytes themselves because the recorded
model cannot see them.

**2.2 A dictionary that closes on a key with no value.** `parseDict` reads a key and then a value; a
`>>` where the value belongs throws, so the object is carried. The walk stops the same way and carries
it. **Matched**; witnessed by `...::a_dictionary_whose_last_key_states_no_value_is_not_read` and the
second property in `pdf_damaged_shapes.rs`.

**2.3 A body with no `endobj` at all.** `tryToParseInvalidIndirectObject` scans to the end of the file
and throws `PDFInvalidObjectParsingError`, which fails the whole load. The walk refuses the file the
same way. **Matched**; witnessed by `pdf/pdf_merge/body_damaged`, declined on both sides.

**2.4 A body nested deeper than the scan builds.** The reference parses to the depth its own stack
holds — measured, a page dictionary holding a value 20 000 deep is parsed and its page turned, one
nested 30 000 is kept as the bytes it states — and past that it carries the object. The engine builds
to `object_depth_max` (30 000) and carries past it. **Stated**: in the band between the two the engine
parses a value the reference keeps as bytes, so a page the reference loses in that band is turned
here. No case: the canonical model parses the carried value itself and aborts, so the band's witness
is `parity/scripts/probe-pdf-bounds.mjs` (three rows) and the crate's own deep-nesting tests.

**2.5 An unbalanced string or an unterminated array.** `parseString` throws on an unbalanced
parenthesis and `parseArray` runs into the end of the file, so the object is carried. The walk's
`literal_string` and array frame answer the same way. **Matched**; witnessed by
`...::an_unbalanced_string_is_carried_as_its_own_bytes`. No case: a corpus fixture for either would
answer exactly as §2.1, which is already witnessed, and the bytes carried are the same bytes.

**2.6 A reference whose halves are not whole numbers.** `parseNumberOrRef` accepts `1.5 0 R` and
builds a reference from the two numbers. `number_or_reference` requires two integers and carries the
body otherwise. **Stated**: no file states a fractional object number, and a reference the format does
not allow is not one this model can key. No case.

## 3. Streams and containers

**3.1 A `/Length` that does not land on `endstream`.** `parseDictOrStream` moves to the stated end,
looks for `endstream` and, finding none, rescans for the marker with nested `stream` keywords counted.
The walk's `stream_after` does the same. **Matched**; witnessed by
`pdf/pdf_rotate/short_length`, `long_length` and `absent_length`.

**3.2 A `/Length` stated as a reference.** The reference reads it with `dict.get`, not `lookup`, so
the entry is not a number and the end-marker scan runs. The walk reads it the same way, on purpose.
**Matched**; witnessed by `pdf/pdf_rotate/short_length` and
`writing::pdf::tests::a_stream_whose_length_names_no_object_is_recovered`.

**3.3 The end-of-line after `stream`.** `matchKeyword` tries the four spellings the format allows
before the bare keyword. The walk tries them in the same order. **Matched**; witnessed by
`...::a_stream_keyword_may_end_its_line_either_way`.

**3.4 A stream with no `endstream`.** `findEndOfStreamFallback` throws `PDFStreamParsingError` when
the nesting never returns to zero, so the object is carried. The walk carries it the same way.
**Matched**; witnessed by `...::a_stream_without_an_end_marker_is_carried_as_its_own_bytes`. No
case: the fixture would answer as §2.1 does, and the bytes carried are the same bytes.

Each such recovery reads to the end of the file, so a file of them costs the square of its own length on
the reference's side: it rescans from every stream it cannot end. The walk reads such a tail once. A
recovery that finds its marker ends the stream where the marker begins and the walk resumes past it, so
the bytes it read are never read by a later recovery; the first one that reaches the file's end without
balancing records every `stream` and `endstream` token the tail holds, the nesting each left and the
token that closes each level, and a recovery beginning inside that tail is answered from the record
without reading: by two lookups, unless it begins inside a recorded token, whose bytes are not a keyword
to that pass — such a recovery reads the tail once itself, as the reference reads it per stream, while
the record keeps answering the positions after it.
`writing::pdf::tests::a_remembered_recovery_answers_what_a_fresh_scan_answers` holds the record to a
fresh scan from every position,
`...::a_recorded_tail_names_the_token_that_closes_every_level` pins that the record answers by lookup
rather than by search, and
`...::a_recorded_tail_answers_every_later_recovery_without_reading` that no later recovery reads.
Measured here, a page followed by 12 800 streams that never end, with one `endstream` at the end of the
file for the recovery to find but never balance against, is read and turned in 37 ms in release (174 ms
in debug), and 102 400 of them in 8.6 MiB in 138 ms (1.3 s in debug) — the cost is the file's length.
The reference takes 4.9 s over 3 200 of them and 62 s over 12 800, where its answer is no result at all
— the probe rows "3 200 streams that never end beside one endstream" and "a file with 400 streams that
never end: the cost grows with them". The record is the one part of the walk that costs more memory than
the bytes it read: a token is 32 bytes and a tail holds one per keyword, so a file dense in `stream`
keywords has a record a few times its own length while the walk runs. One other shape costs more than
the file's length, and it costs the reference the same: a container whose entries all name one offset is
read once per entry, which is `/N` times that entry — §3.18 says so.

**3.5 A container whose objects cannot be read.** `parseIndirectObject` hands a `/Type /ObjStm`
stream to `PDFObjectStreamParser`, which decodes it and assigns each object it holds; a failure
anywhere re-parses the container as invalid bytes, so the container survives and the objects inside it
are gone. The walk does the same — and reports it, because the produced file holds the container
unreachable and nothing else. **Matched on the file, stated in the answer**, in the sentences below,
because the cases know different things:

- a container the walk read says how many of its objects it placed, so the count is the container's own
  `/N` less those — a container that keeps one of its two objects and then fails loses one — and the
  answer prints it under `pdf_page_objects_lost`, "N object(s) the produced file does not hold as an
  object" (a body the index cannot name is one the produced file does not hold as an object, §1.6);
- a container it could not read at all — filters it cannot decode, a count its bytes could not hold, no
  `/First` it can read from (§3.6) — tells it nothing beyond what the file claims, so the answer prints
  that claim under `pdf_page_objects_unread`: "N object(s) stated in a container that could not be read
  are not in the produced file". The number is the container's own `/N`, as the file states it and never
  one this walk counted; a container that states no count at all is not one of these. A fractional count
  is read the way the reference's own counting loop reads it — the entries below the next whole number
  above it, which `pdf_tolerance.rs::a_container_stating_a_fractional_count_reads_both_its_entries`
  pins — and a negative one reads none, as that loop does.

A count written as an integer is read exactly, and the reference reads one written as a real at the
value a double holds for it. This model holds a real at single precision, so a real `/N` above 2^24 is
read at that value — `/N 33554433.0` reads 33554432 — and the answer prints that value, and places the
entries up to it. A container stating such a count and holding that many entries would lose the one the
rounding takes, which needs 128 MiB of decoded bytes at the least (four per entry) — inside
`container_decode_max_bytes`, so the shape is a limit of the object model rather than one this walk
could not meet; no such container was measured, and every container that states a count its bytes could
not hold is carried whole (§3.6) before a single entry is read. A count past what a count holds — a
real that reads as infinity, or the total of several claims — is no number any file states, so the
answer says that instead of printing a ceiling: "more objects are stated in containers that could not
be read than this engine can count, and the produced file holds none of them"
(`pdf_page_objects_unread_over`).

The counts are the file's statements added together, so a merge reports the total of its inputs.

The reference's answer carries no loss field for a PDF action at all. Witnessed by
`...::a_container_that_cannot_be_expanded_is_reported_as_the_loss_it_is`,
`...::a_container_count_past_its_own_bytes_is_still_the_files_own_statement`,
`...::two_container_counts_add_up_to_what_the_two_containers_state`,
`...::a_total_past_what_the_host_holds_is_flagged_rather_than_saturated`,
`...::a_count_past_what_the_host_holds_is_flagged_rather_than_printed_as_the_ceiling`,
`...::a_container_keeps_the_objects_it_read_before_one_that_does_not_parse`,
`...::a_container_whose_first_is_not_a_whole_number_of_bytes_is_carried_and_reported`,
`pdf_tolerance.rs::a_container_stating_a_count_no_container_could_hold_is_turned`,
`pdf_tolerance.rs::a_container_that_decodes_and_states_a_count_past_its_bytes_reports_it`,
`writing::pdf::tests::rotate_carries_a_container_it_cannot_expand`,
`writing::pdf::tests::a_container_count_past_what_the_host_holds_is_said_as_that_fact` and
`writing::pdf::tests::a_merge_states_the_objects_of_every_input_it_could_not_carry`. No corpus case:
the note would differ from the reference's silence, and `coverage.rs` records the marks unreached for
that reason.

**3.6 A container whose `/N` or `/First` does not resolve.** `PDFObjectStreamParser`'s constructor
reads both with `lookup(..., PDFNumber)`, which throws, so the container is carried as the bytes it
states and nothing inside it is placed. The walk answers the same way: a container that states no `/N`,
or one that is not a number, is carried whole with nothing placed, and nothing is reported either —
its bytes are what the produced file holds, so the objects inside it are exactly as unreachable as
they were in the input, and the file states no count for the answer to attribute; one that states a
count but no usable `/First`, or one whose count its bytes could not hold, is carried and reported as
§3.5 says. A `/First` is usable when it names a whole number of bytes, of either sign: the reference
places an entry at that value plus the entry's own offset, so a negative one names a position inside the
bytes as soon as an offset reaches past it. Measured, a container stating `/First 12` places both of
its entries, one stating `/First -4` whose entry names offset 20 places the object at byte 16, and one
stating `/First 12.5` — no position a byte stream can index — is kept as the bytes it states.
**Matched**; witnessed by `...::a_negative_first_reads_the_entry_its_offsets_reach`,
`...::a_container_whose_first_is_not_a_whole_number_of_bytes_is_carried_and_reported`,
`pdf_tolerance.rs::a_container_whose_count_is_not_a_number_is_carried` and, for the index-stream half,
`...::an_index_stream_whose_numbers_do_not_resolve_is_not_adopted`.

**3.7 A container whose later object does not parse.** `parseIntoContext` assigns each object as it
reads it, so the objects before the failure stay in the context. The walk assigns them the same way.
**Matched**; witnessed by
`...::a_container_keeps_the_objects_it_read_before_one_that_does_not_parse`.

**3.8 An index stream.** `parseIndirectObject` hands a `/Type /XRef` stream to
`PDFXRefStreamParser`, which adopts the trailer entries its dictionary states and never assigns the
stream itself; one whose `/Size`, `/W` or `/Index` does not resolve throws, and the object is then
carried as invalid bytes. The walk adopts the entries, drops a stream it could read and carries one it
could not. **Matched**; witnessed by `pdf/pdf_merge/index_stream_absent`,
`...::an_index_stream_whose_bytes_cannot_be_decoded_is_not_adopted` and
`...::an_index_stream_whose_table_is_truncated_still_states_its_entries`.

**3.9 A filter chain past the decode ceiling.** `decodePDFRawStream` decodes stage by stage into a
typed array, so a chain of expanding filters multiplies and dies at the array's own limit; the
container is then carried. The engine stops at `container_decode_max_bytes`, which is that limit, and
carries the container. **Matched**; measured in `parity/scripts/probe-pdf-bounds.mjs` (a five-stage
chain decodes to 2.1 GiB and produces; a six-stage one spends thirty seconds and produces too) and
witnessed by `pdf/pdf_rotate/container_decoding_eighty_mib` for the cost, and by the crate's injected
bounds for the ceiling itself.

**3.10 Flate, cut inside a stored block.** `FlateStream`'s uncompressed-block path sets `eof` and
returns the prefix it read when the bytes run out, so the reference decodes what the file holds and
keeps the container's objects. flate2 reports only that it needs more input, and cannot say which kind
of block was cut, so the engine carries the container and the objects are lost. **Stated**: the
divergence is the engine's, and the answer says so through §3.5's loss. No case: the reference's
answer carries no loss, so a case would compare the note against silence rather than the two files.

**3.11 Flate, cut inside a compressed block.** `getCode` throws when the codes run out, so the
container is carried. The engine carries it too. **Matched**; witnessed by
`pdf/pdf_merge/object_stream_truncated_flate` (cut by four bytes, which is only the checksum, and
still decodes) and `...::a_flate_stream_is_read_up_to_its_last_block`.

**3.12 The Flate checksum and `/DecodeParms`.** Neither side reads the Adler-32 that follows the last
block, and neither applies `/DecodeParms` to `FlateDecode`, because `decodeStream` hands the
parameters only to `LZWDecode`. The engine reads the last block and ignores both. **Matched**;
witnessed by `pdf/pdf_merge/object_stream_truncated_flate` (produced, so the checksum is not
required) and `...::the_decode_parameters_follow_the_shape_of_the_chain`.

**3.13 RunLength, cut inside a run.** `RunLengthStream.readBlock` reads the two bytes a header
occupies and takes the bytes the stream still holds, so a run the bytes do not fill is as long as they
are. The engine does the same. **Matched**; witnessed by
`pdf/pdf_rotate/object_stream_truncated_run_length` and
`...::a_truncated_run_length_stream_decodes_the_bytes_it_states`, and
`...::a_run_length_stream_without_an_end_marker_decodes_its_runs` for a missing end marker.

**3.14 ASCIIHex's odd trailing digit.** `AsciiHexStream` pads a lone nibble only when `>` ended the
data, and drops it when the bytes simply run out. The engine's `ascii_hex` does the same. **Matched**;
witnessed by `pdf/pdf_rotate/object_stream_ascii_hex` and
`...::a_hexadecimal_string_with_an_odd_digit_count_is_padded`.

**3.15 LZW's `/EarlyChange`.** `decodeStream` passes the entry to `LZWStream`. The object model reads
the same entry. **Matched**; no case and no crate test: no fixture states an LZW container, and both
sides hand the parameter to the same decoder from the same place, so a fixture would exercise one
decoder twice rather than compare two answers.

**3.16 A filter neither side reads.** `decodeStream` throws `UnsupportedEncodingError`, so the
container is carried; the object model's decoder fails the same way. **Matched**; witnessed by
`...::a_container_that_cannot_be_expanded_is_reported_as_the_loss_it_is`.

**3.17 A `/Filter` stated indirectly.** `decodePDFRawStream` reads it with `lookup`, so a reference is
the name or the array it names. The engine resolves it through the object map the same way.
**Matched**; witnessed by `...::a_filter_stated_indirectly_is_the_filter_it_names`.

**3.18 A container whose entries all state one offset.** `PDFObjectStreamParser` reads each entry from
the offset its header states, so two entries may name the same bytes: the container is decoded once and
the object is read once per entry. The walk reads it the same way and holds itself to no allowance over
it, because what the expansion costs is the entries the file states on both sides — a budget of the
engine's would refuse this very file. **Matched**; witnessed by
`pdf/pdf_rotate/object_stream_one_offset` (the page turned, nothing lost),
`pdf_damaged_shapes.rs::a_container_whose_entries_state_one_offset_is_produced_by_both_sides`,
`pdf_tolerance.rs::a_container_whose_entries_state_one_offset_is_read`, and the probe row
"a container whose 32 entries state one offset" (measured: the reference produces in a fraction of a
second).

## 4. The index and the trailer

**4.1 An `xref` section that does not parse.** `maybeParseCrossRefSection` is not wrapped in a
`try`, so a table whose entry is not two numbers fails the whole load. The walk parses the section's
syntax for the same reason and refuses the file. **Matched**; witnessed by
`pdf/pdf_rotate/index_syntax_broken`, declined on both sides.

**4.2 An index that cannot be followed.** The reference never uses the offsets: it walks the body.
The engine does the same. **Matched**; witnessed by `pdf/pdf_rotate/shifted_index`,
`absent_index`, `misplaced_index` and `pdf/pdf_rotate/last_definition`.

**4.3 A trailer `/Root` that is not a catalog.** `maybeRecoverRoot` reads `/Root` with a lookup, then
walks every indirect object and takes the last one `isValidCatalog` accepts. That test compares
`dict.lookup('/Type')` — one step — with the name `/Catalog`, while the object itself was built by
`parseDict`, which compares the *stated* `/Type`. So a `/Root` whose `/Type` is a reference naming a
dictionary is no catalog: the lookup answers the dictionary, not the name, and the body's own catalog is
adopted in its place — measured, the reference produces. `Scan::recover_root` compares the stated
`/Type` the same way, so it adopts the body's own catalog too. **Matched** on that shape; witnessed by
`pdf/pdf_rotate/root_recovered`, `pdf/pdf_rotate/two_catalogs` and
`...::a_root_whose_type_is_a_reference_is_not_a_catalog`. A reference that names the *name* `/Catalog`
itself passes `isValidCatalog`, but the object was built as a plain dictionary and the load's
`getPageIndices` asks it for a `Pages` method it has not got, so the load throws and the answer is its
"cannot read … as a PDF" refusal; this engine, comparing the stated entry, adopts the body's catalog
instead and produces. **Stated** on that one: a file the reference refuses and this engine edits. When
neither the trailer's entry nor the body yields a catalog, the load refuses the file, and so does the
engine, with the same sentence.

**4.4 A catalog the trailer states in place.** `maybeRecoverRoot` reads the entry with `lookup`, so a
dictionary is the catalog itself; the writer then writes it back inline, and a file whose `/Root` is
inline is one no reader follows — measured, the reference's own output of such a file cannot be
parsed. The engine deliberately gives the catalog a number of its own, because matching the reference
here would hand the user a file nothing can open. **Stated**: the produced file is a document where
the reference's is not, a divergence that runs in the reader's favour. Witnessed by
`...::a_root_the_file_states_inline_is_read` and
`...::an_inline_catalog_beside_an_object_at_the_number_ceiling_is_turned`; no case, because the
reference's output cannot be canonicalised.

**4.5 An encryption dictionary.** `PDFDocument.load` throws `EncryptedPDFError` when the trailer
states an `/Encrypt` it can resolve. The engine refuses with the password sentence, and names it
before it walks the page tree where the reference resolves the tree first — stated in `page.rs`.
**Matched** on which files are refused, **stated** on which refusal a damaged body gets: a file whose
tree the reference cannot resolve is unreadable to it and needs a password here. Witnessed by
`pdf/pdf_rotate/encrypted` and `pdf/pdf_rotate/encrypted_damaged_index` (whose index damage did *not*
destroy the entry, so both sides answer as encrypted).

**4.6 A catalog whose `/Type` is not the name `/Catalog`.** `PDFObjectParser.parseDict` builds a
`PDFCatalog` only for a dictionary whose own `/Type` is that name, and a plain `PDFDict` for any
other — a catalog with no `/Type`, with another name, or with `/Type` stated as a reference — so the
load refuses the file unless recovery has adopted another catalog (§4.3). The engine requires the
same: the trailer's `/Root`, followed one step, must be a dictionary whose stated `/Type` is
`/Catalog`. **Matched**; witnessed by `pdf/pdf_rotate/catalog_without_type`, declined on both sides,
and by `...::a_root_whose_type_is_a_reference_is_not_a_catalog`.


## 5. The page tree

**5.1 A `/Pages` node stated in place.** `PDFCatalog.Pages` reads the entry with `lookup`, so a
dictionary is the node itself, and `traverse` walks it. The engine walks it too, and adopts it as an
object when it writes, because a file whose page tree is stated in place is one no reader follows.
**Matched on the walk, stated on the output** (see §4.4 for the same repair); witnessed by
`writing::pdf::tests::a_page_tree_stated_in_place_is_walked`; no case, because the reference's output
cannot be canonicalised.

**5.2 A kid that is not a page or a pages node.** `traverse` looks each kid up, recurses when it is a
page tree and hands it to the visitor, which builds a page only for a page leaf — so a number, a
typeless dictionary, a carried object or a reference that names nothing is passed over. The walk
passes over them the same way. **Matched**; witnessed by `pdf/pdf_rotate/page_unparsable` and
`pdf/pdf_merge/page_unparsable`.

**5.3 A kid that is a page dictionary stated in place.** `traverse` hands the dictionary itself to the
visitor, which builds a page from it and then has no reference to name it by, so the save faults. The
engine refuses the file with the reference's own sentence. **Matched**; witnessed by
`pdf/pdf_rotate/page_dictionary_inline`, declined on both sides.

**5.4 A kid that is a page tree stated in place.** `traverse` recurses into it, because the parser
built a `PDFPageTree` for a dictionary whose `/Type` is `/Pages`. The walk walks it too. **Matched**;
witnessed by `writing::pdf::tests::a_page_tree_stated_in_place_is_walked` (the tree it states holds the
page).

**5.5 A node the tree names twice.** `traverse` visits it once per name, so its pages are copied
twice. The walk keeps the path it is on rather than every node it has seen, and visits it twice as
well. **Matched**; witnessed by `pdf/pdf_rotate/page_tree_named_twice` (a diamond, produced with the
page turned twice).

**5.6 A node reached from itself.** `traverse` recurses until the runtime throws
`Maximum call stack size exceeded`, and `loadPdf` turns that into the same "cannot read … as a PDF"
refusal this engine prints. **Matched**; witnessed by `pdf/pdf_rotate/page_tree_names_itself`,
declined on both sides.

**5.7 A node with no `/Kids` array.** `PDFPageTree.Kids` reads the entry with `lookup(..., PDFArray)`,
which throws, so the load fails. The walk refuses the file with the same sentence. **Matched**; no
case: the fixture would answer as §4.1 does, and the refusal is stated where it is made. Witnessed by
`writing::pdf::tests::a_page_tree_that_does_not_resolve_is_refused`.

**5.8 A `/Count` that is not a number.** The reference reads `/Count` only where it inserts or removes
a page, and faults there (`Expected instance of PDFNumber`). The engine produces, taking the pages the
kids state. **Stated**: a permissive divergence, recorded by
`writing::pdf::tests::a_page_less_file_whose_tree_states_a_deep_count_is_turned` and by the probe row
"a page tree whose /Count is not a number"; no case, because the reference's answer is a fault inside
its library rather than a produced file.

**5.9 A `/Parent` path that returns to where it began.** The reference walks `/Parent` to collect the
entries a page inherits and never stops, so the load dies with `Maximum call stack size exceeded` and
the answer is a fault rather than a file. The engine marks the nodes it has passed, so the walk ends
and the page is turned. **Stated**: a permissive divergence, recorded by
`writing::pdf::tests::a_parent_cycle_ends_rather_than_hanging` and
`writing::pdf::tests::a_parent_cycle_that_states_a_box_ends_at_it` and by the probe row
"a page whose /Parent names its own tree"; no case, because the reference's answer is a fault inside its
library rather than a produced file.

**5.10 A tree root whose `/Type` is not the name `/Pages`.** `PDFObjectParser.parseDict` builds a
`PDFPageTree` only for a dictionary whose own `/Type` is that name, and `PDFCatalog.Pages` hands the
raw dictionary to `computePages`, which calls `traverse` on it — so a root with no `/Type`, with
another name, or with `/Type` stated as a reference has no `traverse` to call and the load faults.
The walk asks the same of the root and refuses the file with the same sentence; a *kid* is not held to
it, because `traverse` passes over one it cannot take for a page or a page tree (§5.2). **Matched**;
witnessed by `writing::pdf::tests::a_page_tree_root_that_does_not_state_its_type_is_refused` and the
declined case `pdf/pdf_rotate/page_tree_root_without_type`.

**5.11 A page-tree entry stated as a reference to a reference.** `PDFCatalog.Pages` and
`PDFPageTree.Kids` read their entry with a typed `lookup`, which takes one step: what they find is a
reference, not the dictionary or array the type asks for, so the load faults and the file is refused.
Every read of a file-stated entry takes exactly one step here too: `graph::deref`, `graph::resolved`,
`graph::dictionary` and `graph::deref_mut` are the helpers that make that step, and the page walk, the
resource lookup and the scan's `/N`, `/First` and `/Type` read through them; the form's field tree takes
the same step by hand — a `/Parent` link as one read of the object map, a `/Fields` element through
`graph::dictionary` — so the walk refuses the file with the reference's own "cannot read … as a PDF"
sentence.
**Matched**; witnessed by `pdf/pdf_rotate/page_tree_kids_by_reference` and
`pdf/pdf_rotate/page_tree_root_by_reference` (declined on both sides),
`pdf_tolerance.rs::a_page_tree_entry_stated_as_a_reference_to_a_reference_is_refused`,
`pdf_damaged_shapes.rs::a_chained_page_tree_entry_is_refused_by_both_sides`, and the probe row
"a /Kids stated as a reference to a reference".

**5.12 A box entry stated as a reference to a reference.** `PDFPageLeaf.MediaBox` reads its entry with
a `lookup` that is not a `lookupMaybe`, so a reference that names a reference is not four numbers and
the library's own type check faults inside the operation. The engine takes the same one step and faults
the same way: an internal failure, not a refusal. **Matched**; witnessed by
`pdf/pdf_resize/media_box_by_reference` (declined on both sides as a third-party failure),
`pdf_tolerance.rs::a_box_entry_stated_as_a_reference_to_a_reference_faults`,
`pdf_damaged_shapes.rs::a_chained_box_entry_faults_on_both_sides`, and the probe row
"a /MediaBox stated as a reference to a reference". The box is read only by the operations that need
it: a turn does not read `/MediaBox`, so a chained box under `pdf_rotate` is produced by both sides.
`/CropBox`, `/Rotate`, `/Annots` and `/Parent` are `lookupMaybe`/`lookup` reads of the same one step —
absent, null or a reference that names nothing is "none", and a value of another type is the library's
fault.

**5.13 A catalog with no `/Pages` entry.** `PDFCatalog.Pages` reads the entry with
`lookup(..., PDFDict)`, which throws when the entry is absent or names nothing, so the load fails. The
walk refuses the file with the same sentence. **Matched**; no case and no crate test: the fixture
would answer as §4.1 does — a file both sides refuse with the unparseable sentence — so no answer
could tell the two refusals apart.

**5.14 A box entry stated as a null.** `/CropBox` is read with `lookupMaybe`, which answers "no crop"
for a null the entry states, so the page shows its media box; `/MediaBox` is read with a typed
`lookup`, which answers nothing for a null and throws, so such a page faults. The engine's `area`
answers both the same way: an absent entry, a value that names nothing and a null are `None`, and the
media box's caller faults on `None`. **Matched**; witnessed by
`pdf_tolerance.rs::a_null_crop_box_falls_back_to_the_media_box`,
`pdf_damaged_shapes.rs::a_null_crop_box_falls_back_to_the_media_box_on_both_sides` (both sides crop to
the same box) and the probe row "a page whose /CropBox is null: the crop falls back to the media box".
No case: the two produced files spell the box they wrote in their own ways, and the crate's and the
reference's differ outside what a recorded entry compares.

**5.15 A form or a page's resources stated as a reference to a reference.** The one-step rule of §5.11
is not the page tree's alone: `PDFCatalog.lookupMaybe('/AcroForm', PDFDict)` and a leaf's
`lookupMaybe('/Resources', PDFDict)` take one step too, find a reference, and throw
`Expected instance of PDFDict, but got instance of PDFRef` — measured, the probe rows "a /Resources
stated as a reference to a reference" and "an /AcroForm stated as a reference to a reference". The
engine reads both through the same one step, so `pdf_text` and `pdf_form_fill` fault with an internal
failure rather than following the chain. **Matched**; witnessed by
`pdf_tolerance.rs::a_page_whose_resources_name_a_reference_faults`,
`pdf_tolerance.rs::a_form_reached_through_a_chained_reference_faults` and
`pdf_damaged_shapes.rs::a_chained_acroform_reference_faults_on_both_sides` and
`pdf_damaged_shapes.rs::a_chained_resources_reference_faults_on_both_sides`. No case: the reference's
answer is a fault inside its library rather than a produced file.

**5.16 The field tree, read one step at a time.** `PDFAcroForm.getFields` builds every field before
a value is applied. `Fields()` reads the form's `/Fields` entry with `dict.lookup`, one step, so a list
stated as a reference to an array is the same list; each element is then read with
`Fields.lookup(idx, PDFDict)`, a typed read that throws for an element that names nothing, names a
further reference, or names a value that is not a dictionary. `createPDFAcroField` decides a container
by `isNonTerminalAcroField`, whose `/Kids` read is the same one step; a `/Kids` element must be a
*reference* to a dictionary, because `createPDFAcroFields` keeps a kid only when the array states one,
while a `/Fields` element that is a dictionary stated in place is a field — the element's own value is
the dictionary. A kid that is not a dictionary at all is the fault the typed widget read throws, for
every kind whose appearance a fill regenerates, and a field that states an empty `/Kids` array draws no
widget. The container test reads `/Kids` untyped, so an entry that is not an array is no list of kids
there, while `Kids()` — the typed read the appearance phase takes — throws on it. Measured, the probe
rows "a /Fields entry that names the array", "a /Kids entry that names the array", "a /Kids entry that
is a field dictionary in place", "a text field whose `/Kids` states a number", "a text field whose
`/Kids` is a number" and "a text field whose `/Kids` is an empty array". The engine reads both arrays
with the same one step, registers a dictionary stated in place only where it is a field, fills no kid a
container states in place, faults on a kid that is not a dictionary, and reports an entry that is not an
array as the fault the widget read raises. **Matched**; witnessed by
`form::tests::a_fields_entry_that_names_an_array_is_read`,
`...::a_kids_entry_that_names_an_array_is_read`,
`...::a_kid_stated_in_place_is_no_field_of_the_container`,
`...::a_terminal_with_an_empty_kids_array_draws_no_widget`,
`...::a_kid_that_is_not_a_dictionary_is_the_widget_reads_fault`,
`...::a_field_whose_kids_is_no_array_is_the_widget_reads_fault`,
`...::a_signature_reads_its_kids_only_to_flatten`,
`...::a_fields_dictionary_in_place_and_a_field_with_no_parent_are_still_read` and, on both sides,
`pdf_damaged_shapes.rs::a_form_whose_list_is_stated_through_a_reference_is_filled_by_both_sides`,
`...::a_kid_stated_in_place_is_no_field_on_either_side`,
`...::a_kid_that_is_not_a_dictionary_faults_on_both_sides`,
`...::a_kids_entry_that_is_not_an_array_faults_on_both_sides` and
`...::a_field_with_no_widgets_writes_no_appearance_on_either_side`. No case: the model reads the same
fields of the two produced files, and a fault inside the reference's library has no file to compare.
Three entries of the two produced files differ where no reader of the canonical model looks: where a
field, a widget or a `/Fields` element is a dictionary stated in place, the entry that names it is
written as the reference the engine registered; where a `/Fields` or `/Kids` entry states a
reference to an array, the engine writes the array itself into the entry, leaving the array object
unreferenced; and a `/Ff` real the model's float rounded is written back as the float's spelling
where the fill leaves it as the file stated.

The entries a field states are read with the same one-step typed lookups, each answering the fault that
lookup throws. `/FT` — `lookup(…, PDFName)`, whose lookup has no absent case and is read for every
terminal field while the form is built — faults for a field whose chain states no type at all, and for
one that states a value of another type: a value that names nothing, or a further reference. A `null`
is not one of those — `PDFDict.get` answers `undefined` for it, which the inheritance skips — so a
field whose own type is null takes the one its parent states, and only a chain that states none
faults. `/Ff` — `lookupMaybe(…, PDFNumber)` — decides what a `/Ch` or a `/Btn` is, is read for a
text field by the appearance a fill regenerates, and is read again for an option list's `Sort` bit while
its appearance lists the options, so the engine reads it in exactly those places: a `/Tx` field the
request does not fill, whose appearance the file already states, has its flags read by neither side, and
a `/Ff` stated as a reference that names nothing is no flags at all rather than the fault a typed read
would raise. The number is tested with JavaScript's `&`, which converts both operands to a signed
32-bit integer: a negative real sets every bit (`-1.5` is `-1`), a value past 2^32 keeps only its low
bits, and a value no integer holds is zero. The engine reads the flags through the same conversion
(`form::to_int32`), and `createPDFAcroButton`'s order is kept too — a `/Btn` tests the push-button bit
before the radio bit, so a field stating both is a push button. Measured, the probe rows "a choice
field whose flags are a negative real", "a button whose flags are a negative real", "a choice field
whose flags are a huge real" and "a choice field whose flags are a whole number past 2^53". One shape
inside it is **stated**: the reference reads the decimal as a double, while this model holds a real
as a 32-bit float, so a `/Ff` real past what the float states exactly can land on another bit.
Measured, `4294967295.5` is `-1` (a combo box, produced) there and the float rounds it to 2^32, whose
low 32 bits are zero (a list box, refused) here. Witnessed by
`pdf_damaged_shapes.rs::a_flags_real_past_the_float_is_read_differently_by_the_two_sides` and the
probe row "a choice field whose flags are a real past the float". A whole number has no such band up
to `i64`: it is exact in both models up to 2^53, and from there the engine reads the double the
digits state, which is the number the reference reads; past `i64` the object is carried (§6.3), so a
`/Fields` entry that is such a field is the fault the typed read raises and a kid is skipped.
`/Opt` is `Opt()` —
`this.dict.lookupMaybe(…, PDFString, PDFHexString, PDFArray)`, on the field's own dictionary,
ascending nothing — so a `/Opt` its parent states is none of the field's, and both option readers (the
one the appearance lists and the one a radio group's export values are read by) go through it. Measured,
a choice field whose only `/Opt` its parent states answers `offers no choice to select` and is listed
as no fillable field; witnessed by
`form::tests::a_choice_field_takes_its_options_from_its_own_dictionary_only`. `/T` is
`lookupMaybe(…, PDFString, PDFHexString)` on the field's own dictionary — no inheritance — so an
entry that is absent, null or a reference that names nothing is no name at all, while a name, a number
or a reference to a reference is the fault it throws. **Matched**, but for the two `/T` shapes stated
below; witnessed by `form::tests::a_field_that_states_no_type_is_the_references_fault`,
`...::a_field_type_that_names_a_reference_is_the_references_fault`,
`...::a_field_flags_entry_that_names_a_reference_is_the_references_fault`,
`...::a_text_field_whose_flags_name_a_reference_faults_by_its_appearance`,
`...::a_signature_never_reads_its_flags`,
`...::a_field_of_a_type_the_reference_knows_no_field_of_is_not_a_field`,
`...::a_name_stated_as_a_reference_that_names_nothing_is_no_name`,
`...::a_flags_entry_that_names_nothing_is_no_flags` and, on both sides,
`pdf_damaged_shapes.rs::a_clean_field_whose_flags_are_a_reference_is_no_fault_on_either_side`,
`...::a_flags_entry_that_names_nothing_is_no_fault_on_either_side`. Measured,
the probe rows "a signature whose `/Ff` is a reference to a reference" and "a clean text field whose
`/Ff` is a reference" produce, and "a filled text field whose `/Ff` is a reference" faults. No case: the
answer is a fault inside the reference's library.

`/MaxLen` is `lookupMaybe(…, PDFNumber)` on the field's own dictionary, so a real is a length to the
reference: measured, a six-character value set on a text field stating `/MaxLen 3.5` faults with
`maxLength=3.5` in the sentence, and one stating `/MaxLen 3.0` faults with `maxLength=3`. The engine
reads the entry as a whole number, so a real is no bound here and the value is written where the
reference faults. **Stated**: a permissive divergence, witnessed by
`pdf_damaged_shapes.rs::a_real_max_length_bounds_the_reference_and_not_the_crate` and the probe rows
"a text field whose /MaxLen is a real" and "a text field whose /MaxLen is a fraction". No case: the
reference's answer is a fault inside its library rather than a produced file.

`createPDFAcroField` reads what a field inherits by ascending its `/Parent` chain, and the reference
ascends that chain with three readers. The constructor's `ascend` reads each link with
`lookupMaybe(…, PDFDict)`, which follows a dictionary stated in place, ends at a link that names
nothing, and throws for a link that names a value that is not a dictionary; it keeps the first entry
that is not falsy, which is why a stated `null` is skipped rather than ending the search (`PDFDict.get`
answers `undefined` for one). `getParent` — the ascent every later read of an inherited entry takes
(`V()`, `Ff()`) and the link a *name* is built through — follows only a reference that names a
dictionary, and a link that names nothing is the fault its typed `lookup(…)` throws. `Opt()` ascends
not at all, reading the field's own dictionary. So a `/Parent` stated as a dictionary in place
is followed for the type and the flags a field is built with, and is no parent at all for its name;
and a `/Parent` that names nothing ends the build's ascent where it faults a name read. The reference
reads a name for the field the request names, for every field the catalogue lists, and for every field
whose appearance it updates — which is every field of a kind a fill can take. **Matched**; witnessed by
`form::tests::a_null_type_is_inherited_from_the_parent_that_states_one`,
`...::a_parent_link_that_names_nothing_is_the_references_fault`,
`...::a_parent_link_that_names_a_reference_is_the_references_fault`,
`...::a_fields_entry_that_names_a_reference_is_the_references_fault`,
`...::a_field_name_that_names_a_reference_is_the_references_fault`,
`...::a_parent_stated_in_place_is_followed_for_the_entries_a_field_inherits` and, on both sides,
`pdf_damaged_shapes.rs::a_field_read_past_a_null_entry_is_filled_by_both_sides`,
`...::a_parent_stated_in_place_is_filled_by_both_sides`. Measured, a form whose
text field holds a `/Parent` that names nothing faults whatever the request asks for, and one whose
*signature* field holds it faults where the request reaches that field or the catalogue is built. No
case: the answer is a fault inside the reference's library.

An option list's `Sort` bit is read by the *later* reader: `isSorted` is `hasFlag(Sort)`, which is
`getFlags` and so `getParent`. A flag the field states itself, or a parent a reference names, sorts the
list its appearance draws; one a parent states in place does not. Measured on the reference's own
appearance streams for `/Opt [(b) (a)]`: "a" is drawn above "b" for the first two and "b" above "a" for
the third. **Matched**; witnessed by
`form::tests::a_sort_flag_is_read_through_the_parent_link_that_states_it`. No case: the appearance
stream is the engine's own, and the canonical model reads the field's entries rather than its drawing.

The name is the chain `/Parent` states, not the tree the walk reaches through `/Kids`:
`getFullyQualifiedName` joins each link's own `/T` with `.`, and a link that states none contributes the
word `undefined` — so a child under a named parent that states no `/T` of its own is addressed as
`P.undefined`, one whose parent states no name is `undefined.Q`, and a child whose `/Parent` is absent
is named by itself. The engine names a field the same way. **Matched**; witnessed by
`form::tests::a_name_comes_from_the_parent_chain_and_an_absent_link_states_undefined`, and measured
by the probe row "a field named through the chain its `/Parent` states", which the reference fills
under the name `P.undefined`.

The widget list is the one place the fault is deferred rather than raised while the tree is built:
`updateFieldAppearances` reads it after the values are applied, so a malformed kid beside a request that
names no field is the usage refusal that lists the form's fields on both sides, not a fault. The engine
records the fact and raises it there. Measured, the probe row "a bad widget kid beside a request naming
no field". **Matched**; witnessed by
`pdf_damaged_shapes.rs::a_bad_widget_kid_does_not_turn_a_missing_field_into_a_fault`.

A kid whose `/T` is a stated `null` is a widget, not a field: `PDFDict.has` answers false for a null,
so the container test finds no field of it and the typed widget read lists it. Measured, the
reference's own output for such a kid carries an appearance stream — 4 253 bytes against the 3 783 of
the same field whose `/Kids` is empty, which draws none. **Matched**; witnessed by
`form::tests::a_kid_whose_name_is_a_stated_null_is_a_widget`. No case: the appearance stream is the
engine's own, so no reader of the canonical model compares it.

The tree is walked from a worklist rather than by recursion, so a tree nests as deep as the file
states. A depth bound cannot stand in for that: measured, the reference fills a 20 000-deep tree in
0.80 s, while a recursive walk of a thousand levels ends the process rather than answering it on the
2 MiB stack the harness gives the engine, so any bound low enough to fit the stack refuses a tree the
reference fills. A kid a container names twice is no cycle and is a field of that container twice, as
the reference's own walk makes it. **Matched**; witnessed by
`pdf_tolerance.rs::a_field_tree_twenty_thousand_deep_is_filled_on_the_harness_stack` and
`form::tests::a_kid_a_container_names_twice_is_a_field_of_it_twice`. No case: the produced files hold
the same one field. A kid that names one of its own ancestors is not this shape: it is §5.17, where the
reference cannot answer at all.

`/FT`'s fault is unconditional on both sides — measured, even a request that names an empty field
faults, because the refusal's catalogue builds the same tree. An empty name is matched in its two other
places too: the kit tests a requested name before it asks the form for one, so an empty one is the
refusal that names it `""`, and its catalogue skips an empty name rather than listing it. The engine
does both. **Matched**; witnessed by
`pdf_tolerance.rs::an_empty_field_name_is_never_a_field_and_is_never_listed`. No case: the corpus's
refusal for a missing field already names the same sentence over a form with no nameless field, and a
case carrying an empty name would differ only in the name it prints.

A terminal field of a type the reference knows no field of — `/FT /Foo` — is a `PDFAcroTerminal` there
and converts to no field at all, so it is neither listed nor addressable; the engine drops it from its
field list for the same reason. **Matched**; witnessed by
`form::tests::a_field_of_a_type_the_reference_knows_no_field_of_is_not_a_field`.

Five shapes are **stated**, and they come of four things: reading a name permissively, reading a
field's own entries while the tree is built rather than where the request reaches them, holding a real
as a 32-bit float where the reference holds a double, and reading `/MaxLen` only as a whole number.

- a `/T` stated as a *name*. `lookupMaybe(…, PDFString, PDFHexString)` throws on a name, where this
  engine reads the name as the text it states, so a field named that way is addressable here. Measured,
  a fill that addresses that field faults there and produces here. Witnessed by
  `form::tests::a_field_name_stated_as_a_name_is_read_as_its_text`.
- a `/T` that is not a string at all, on a field the request does not reach. The engine reads every
  field's name while it builds the tree, where the reference reads a name only where `getFieldMaybe`
  walks up to it — measured by the probe row "a `/T` that is not a string after the field the request
  names", which the reference produces for, and by the same form with that field first, which faults.
  Witnessed by `form::tests::a_field_name_that_names_a_reference_is_the_references_fault` for the fault
  itself.
- a signature's `/Parent` link that names nothing, on a field the request does not reach: a signature's
  appearance is never updated, so the reference reads neither `/V` nor `/Ff` through that link, and it
  produces for the field the request names — measured. The engine names every field, so it faults.
- a `/Ff` real past what a 32-bit float states exactly: the reference reads the decimal as a double
  and converts that, so `4294967295.5` is `-1` — a combo box, which produces — where this model's
  float rounds it to 2^32, whose low 32 bits are zero (a list box, which refuses). Measured, the probe
  row "a choice field whose flags are a real past the float"; witnessed by
  `pdf_damaged_shapes.rs::a_flags_real_past_the_float_is_read_differently_by_the_two_sides`.
- a `/MaxLen` stated as a real: the reference reads it as a number and faults a value longer than it,
  printing the real in the sentence, where this engine reads only a whole number and writes the
  value. Measured, the probe rows "a text field whose /MaxLen is a real" and "a text field whose
  /MaxLen is a fraction"; witnessed by
  `pdf_damaged_shapes.rs::a_real_max_length_bounds_the_reference_and_not_the_crate`.

The second and third are the same eagerness: a form the reference answers and this engine faults on.
No case for any of the five: a case compares answers that differ.

One more shape is **stated**, and it is the form's own list rather than a field's: a `/Fields` entry that
is not an array at one step — absent, a dictionary, a name, a reference to anything but an array — is
left as the file states it, where the reference's `normalizedEntries` replaces it with an empty array
before it fills. Neither side fills a field from it, so the answer is the same and only the produced
entry differs, which is
`form::tests::a_fields_entry_that_is_not_an_array_fills_nothing_and_is_left_as_stated`. No corpus case
can tell: the field list the model reads is empty on both sides.

**5.17 A field whose kid list or parent link returns to itself.** `PDFForm.getFields` flattens a
container by recursing on its kids (`PDFAcroForm.getAllFields`'s `pushFields`), `getFullyQualifiedName`
builds a name by recursing on `/Parent`, and the constructor's inherited-entry `ascend` recurses on the
same link. A kid that names one of its own ancestors, or a `/Parent` that returns to where it began, is
therefore a tree the reference cannot walk: measured, both shapes die with `Maximum call stack size
exceeded` and the answer is a fault, the probe rows "a form whose field kid list returns to its own
container" and "a form whose field parent link returns to itself". The engine's walk keeps the chain of
fields it is inside and ends at one that names an ancestor, and its name and inherited-entry reads keep
the links they have passed and end at a repeat, so it answers: a form whose only fields are the cycle
holds none, and the request is the usage refusal that names the empty catalogue, while a cycle the
walk can leave yields the fields it reaches. **Stated**: a permissive divergence, and the stop is a
bound of this engine's own — the measured reference failure on the same shape is what justifies it,
as that failure is also what rules out a depth bound in §5.16. Witnessed by
`pdf_damaged_shapes.rs::a_field_kids_cycle_faults_only_on_the_reference` and
`...::a_field_parent_cycle_faults_only_on_the_reference`,
`pdf_tolerance.rs::a_field_kids_cycle_ends_where_the_reference_dies` and
`...::a_field_parent_cycle_ends_where_the_reference_dies`, and
`form::tests::a_field_tree_is_walked_past_the_readings_bound_and_stops_at_a_cycle` and
`...::a_name_is_read_along_a_chain_past_the_readings_bound`. No case: the reference's answer is a fault
inside its library rather than a produced file.

## 6. Numbers and names

**6.1 A name with `#xx` escapes.** `PDFName.of` decodes `#` followed by two upper-case hexadecimal
digits, and only that. `decode_name` does the same. **Matched**; witnessed by
`...::a_name_is_decoded_the_way_the_reference_decodes_one` and
`writing::pdf::tests::a_name_the_file_escaped_is_written_escaped_again`.

**6.2 A number with a leading sign, a leading point, or a trailing point.** `parseRawNumber` accepts
`+`, `-` and `.` as the first byte. `Scan::number` accepts the same set. **Matched**; witnessed by
`...::a_number_may_start_with_a_sign_or_a_point`. No case: no fixture states one, and the value is read
the same either way, so no answer differs.

**6.3 A number past what a double holds.** `parseRawNumber` warns and keeps the value unless the
caller asked for capping, which the kit does not, so a twenty-digit coordinate is a number to the
reference. The engine keeps a whole number as `i64` and a fraction as `f32`: a whole number past
`i64` in an object's body carries the object, and a number in an object *header* saturates instead
(§1.5), because an object's number is one this model can renumber and a coordinate is not. **Stated**:
a page is laid out in points, so the band exists only for numbers no page uses. No case.

**6.4 An object numbered at the format's ceiling.** The reference keys the number as stated and writes
`4294967295 0 obj` back, with an index section beginning there; the model here keys it as stated too.
An object a copy adds is numbered by `graph::free_number` (§1.10): the number past every one the file
states or names, or — when the file states the format's own ceiling — the lowest number it neither
states nor names, because no index can state a number past the ceiling. **Matched**; witnessed by
`pdf/pdf_rotate/object_numbered_past_a_million`,
`...::an_inline_catalog_beside_an_object_at_the_number_ceiling_is_turned` and
`pdf_tolerance.rs::placing_many_objects_past_the_model_costs_the_objects_they_are`.

## What is not in this list

A file whose bytes do not begin with a header anywhere (§1.1's failure) is refused by both sides and
keeps the answer it had; that is not tolerance, so it is not an entry. A genuinely encrypted file *is*
one (§4.5), because the two sides reach the password by different routes: the engine names it before
it walks the page tree, where the reference resolves the tree first, so a file whose body is damaged
is refused as needing a password here and as unreadable there. The entries that end in a refusal are
§2.3, §4.1, §4.3, §4.5, §4.6, §5.3, §5.6, §5.7, §5.10, §5.11, §5.13 and §5.17 — that is all of
them: shapes both sides refuse, or a shape one side refuses where the other answers with a fault or a
file, each here because it does. (§5.17's refusal is the usage answer to a cycle that yields no
field; a cycle the walk can leave is a produced file, and the reference's answer is its fault either
way.) An entry whose *engine* answer is a fault (§5.12 and §5.15) is not one of these: the answer is
the engine's own internal failure, which is what the reference's library raises there too. §5.14 and
§5.16 are mixed entries rather than refusals or faults: §5.14's null `/CropBox` is no crop and
produces on both sides while its null `/MediaBox` is the fault the typed read raises; §5.16's
matched shapes answer as the reference answers — most produce, some refuse with the same sentence,
and the ones whose typed reads throw fault on both sides — and its stated ones, listed under "The
entries whose answers differ", are the shapes where the two sides' answers or produced entries part.
