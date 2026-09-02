//! Tests for [`super`]: what may be merged into one console message, and what
//! must never be.

use super::*;
use crate::process::Stream;

/// A batch as `(stream, text)`, for terse assertions.
fn batch(taken: Option<OutputBatch>) -> Option<(Stream, String)> {
    taken.map(|b| (b.stream, b.text))
}

#[test]
fn a_lone_chunk_is_held_rather_than_emitted_immediately() {
    let mut coalescer = Coalescer::new();
    // Nothing is emitted on the way in: holding is the whole point, and the
    // caller flushes when the adapter has gone quiet.
    assert_eq!(batch(coalescer.push(Stream::Stdout, "hello")), None);
    assert_eq!(
        batch(coalescer.take()),
        Some((Stream::Stdout, "hello".to_string()))
    );
}

#[test]
fn taking_twice_does_not_repeat_the_text() {
    let mut coalescer = Coalescer::new();
    coalescer.push(Stream::Stdout, "once");
    assert!(coalescer.take().is_some());
    // A second flush with nothing new must be silent, or a quiet adapter would
    // reprint its last line on every idle tick.
    assert_eq!(batch(coalescer.take()), None);
}

#[test]
fn consecutive_chunks_on_one_stream_merge_in_order() {
    let mut coalescer = Coalescer::new();
    assert_eq!(batch(coalescer.push(Stream::Stdout, "warn: ")), None);
    assert_eq!(batch(coalescer.push(Stream::Stdout, "one\r\n")), None);
    assert_eq!(batch(coalescer.push(Stream::Stdout, "info: two\r\n")), None);
    assert_eq!(
        batch(coalescer.take()),
        Some((Stream::Stdout, "warn: one\r\ninfo: two\r\n".to_string()))
    );
}

#[test]
fn a_change_of_stream_flushes_rather_than_mixing_the_two() {
    let mut coalescer = Coalescer::new();
    coalescer.push(Stream::Stdout, "on stdout\n");
    // Merging the two would relabel one of them, and the console ranks a line's
    // severity by its *stream* when the line carries no marker of its own.
    assert_eq!(
        batch(coalescer.push(Stream::Stderr, "on stderr\n")),
        Some((Stream::Stdout, "on stdout\n".to_string()))
    );
    assert_eq!(
        batch(coalescer.take()),
        Some((Stream::Stderr, "on stderr\n".to_string()))
    );
}

#[test]
fn the_batch_is_capped_so_one_message_cannot_grow_without_bound() {
    let mut coalescer = Coalescer::new();
    let chunk = "x".repeat(MAX_BATCH_BYTES / 2);
    assert_eq!(batch(coalescer.push(Stream::Stdout, &chunk)), None);
    assert_eq!(batch(coalescer.push(Stream::Stdout, &chunk)), None);
    // The third would exceed the cap, so what is pending comes out first and
    // the new chunk starts the next batch.
    let flushed = batch(coalescer.push(Stream::Stdout, &chunk)).expect("a full batch");
    assert_eq!(flushed.0, Stream::Stdout);
    assert_eq!(flushed.1.len(), MAX_BATCH_BYTES);
    assert_eq!(
        batch(coalescer.take()),
        Some((Stream::Stdout, chunk.clone()))
    );
}

#[test]
fn a_single_chunk_larger_than_the_cap_is_never_split() {
    let mut coalescer = Coalescer::new();
    let huge = "y".repeat(MAX_BATCH_BYTES * 3);
    // Splitting is the tempting thing and it is wrong twice over: a cut inside a
    // multi-byte character corrupts it, and a cut mid-line makes the console
    // rank half a line on its own. An oversized chunk travels whole.
    assert_eq!(batch(coalescer.push(Stream::Stdout, &huge)), None);
    assert_eq!(batch(coalescer.take()), Some((Stream::Stdout, huge)));
}

#[test]
fn an_empty_chunk_neither_flushes_nor_creates_a_batch() {
    let mut coalescer = Coalescer::new();
    // An adapter that sends `"output": ""` must not cause a console message, and
    // must not split the batch either side of it.
    assert_eq!(batch(coalescer.push(Stream::Stdout, "")), None);
    assert_eq!(batch(coalescer.take()), None);

    coalescer.push(Stream::Stdout, "a");
    assert_eq!(batch(coalescer.push(Stream::Stderr, "")), None);
    coalescer.push(Stream::Stdout, "b");
    assert_eq!(
        batch(coalescer.take()),
        Some((Stream::Stdout, "ab".to_string()))
    );
}

#[test]
fn a_multi_byte_character_is_measured_in_bytes_and_kept_intact() {
    let mut coalescer = Coalescer::new();
    // "é" is two bytes. Filling to one byte under the cap and then pushing it
    // must flush rather than half-fit it.
    let filler = "z".repeat(MAX_BATCH_BYTES - 1);
    coalescer.push(Stream::Stdout, &filler);
    let flushed = batch(coalescer.push(Stream::Stdout, "é")).expect("a flush");
    assert_eq!(flushed.1, filler);
    assert_eq!(
        batch(coalescer.take()),
        Some((Stream::Stdout, "é".to_string()))
    );
}

#[test]
fn pending_reports_whether_a_flush_would_produce_anything() {
    let mut coalescer = Coalescer::new();
    // The caller uses this to avoid an idle timer tick doing any work at all.
    assert!(!coalescer.pending());
    coalescer.push(Stream::Stdout, "something");
    assert!(coalescer.pending());
    coalescer.take();
    assert!(!coalescer.pending());
}
