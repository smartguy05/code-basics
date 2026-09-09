//! What a bounded log lost, and how a reader learns it. Included by `ring.rs`.

use super::*;

fn filled(capacity: usize, count: u64) -> Ring<u64> {
    let mut ring = Ring::new(capacity);
    for i in 1..=count {
        ring.push(i);
    }
    ring
}

#[test]
fn sequences_start_at_one_so_zero_is_a_usable_seen_nothing_cursor() {
    let mut ring = Ring::new(4);
    assert_eq!(ring.latest_seq(), 0);
    assert_eq!(ring.push("a"), 1);
    assert_eq!(ring.push("b"), 2);
    assert_eq!(ring.since(0).entries, vec![&"a", &"b"]);
}

#[test]
fn a_full_ring_evicts_the_oldest_and_counts_it() {
    // The first lie a capped log tells: ten rows with four hundred behind them.
    let ring = filled(3, 10);
    assert_eq!(ring.len(), 3);
    assert_eq!(ring.dropped(), 7);
    assert_eq!(ring.since(0).entries, vec![&8, &9, &10]);
}

#[test]
fn sequences_are_never_reused_after_eviction() {
    // They are the cursor, not an index, so eviction must not renumber.
    let ring = filled(2, 5);
    assert_eq!(ring.latest_seq(), 5);
    // Only 4 and 5 survive, so a cursor of 4 yields exactly one entry.
    assert_eq!(ring.since(4).entries, vec![&5]);
}

// ---------------------------------------------------------------------------
// The gap — the failure this module exists for
// ---------------------------------------------------------------------------

#[test]
fn a_cursor_pointing_into_the_dropped_region_reports_the_gap() {
    // The worse of the two lies: a reader asking for everything after 2, in a
    // ring whose oldest surviving entry is 8, must not be handed 8..10 as
    // though 3..7 never existed.
    let ring = filled(3, 10);
    let slice = ring.since(2);
    assert_eq!(slice.entries, vec![&8, &9, &10]);
    assert_eq!(slice.missed, 5, "entries 3,4,5,6,7 are gone");
}

#[test]
fn a_cursor_that_lost_nothing_reports_no_gap() {
    let ring = filled(10, 5);
    let slice = ring.since(2);
    assert_eq!(slice.entries, vec![&3, &4, &5]);
    assert_eq!(slice.missed, 0);
}

#[test]
fn a_first_read_of_an_already_trimmed_ring_is_not_exempt_from_the_gap() {
    // A reader attaching to a page that has been open an hour has missed the
    // interesting part, and a zero cursor must not hide that.
    let ring = filled(3, 100);
    let slice = ring.since(0);
    assert_eq!(slice.entries.len(), 3);
    assert_eq!(slice.missed, 97);
}

#[test]
fn a_read_at_the_exact_eviction_boundary_reports_no_gap() {
    // Off-by-one guard: entries 1..7 are gone and 8..10 survive, so a cursor of
    // 7 has lost nothing at all. Reporting 1 here would cry wolf on every read.
    let ring = filled(3, 10);
    let slice = ring.since(7);
    assert_eq!(slice.entries, vec![&8, &9, &10]);
    assert_eq!(slice.missed, 0);
}

#[test]
fn a_read_one_before_the_boundary_reports_exactly_one_missed_entry() {
    let ring = filled(3, 10);
    assert_eq!(ring.since(6).missed, 1);
}

#[test]
fn an_untrimmed_ring_never_reports_a_gap_at_any_cursor() {
    let ring = filled(20, 12);
    for cursor in 0..=15 {
        assert_eq!(ring.since(cursor).missed, 0, "cursor {cursor}");
    }
}

#[test]
fn an_empty_ring_reports_nothing_and_no_gap() {
    let ring: Ring<u64> = Ring::new(4);
    let slice = ring.since(0);
    assert!(slice.entries.is_empty());
    assert_eq!(slice.missed, 0);
    assert_eq!(slice.next_cursor, 0);
}

#[test]
fn a_cursor_ahead_of_the_ring_returns_nothing_and_no_gap() {
    // A stale reader (a page reload, or a fabricated value) names entries that
    // do not exist yet. That is not a loss and must not be reported as one.
    let ring = filled(10, 5);
    let slice = ring.since(99);
    assert!(slice.entries.is_empty());
    assert_eq!(slice.missed, 0);
}

#[test]
fn next_cursor_never_moves_backwards() {
    // Rewinding a stale reader would make it re-read entries it has already
    // reported, which looks exactly like the page repeating itself.
    let ring = filled(10, 5);
    assert_eq!(ring.since(99).next_cursor, 99);
    assert_eq!(ring.since(0).next_cursor, 5);
    assert_eq!(ring.since(5).next_cursor, 5);
}

#[test]
fn following_next_cursor_reports_each_entry_exactly_once() {
    // The realistic loop, and the property that matters: no duplicates, no
    // silent holes while the reader keeps up.
    let mut ring = Ring::new(100);
    let mut cursor = 0;
    let mut seen = Vec::new();
    for round in 0..5u64 {
        for i in 0..4u64 {
            ring.push(round * 4 + i);
        }
        let slice = ring.since(cursor);
        assert_eq!(slice.missed, 0);
        seen.extend(slice.entries.iter().copied().copied());
        cursor = slice.next_cursor;
    }
    assert_eq!(seen, (0..20).collect::<Vec<u64>>());
}

#[test]
fn a_reader_that_falls_behind_reports_its_gap_once_and_then_catches_up() {
    // Following `next_cursor` past the gap is what stops the same gap being
    // re-reported forever.
    let mut ring = Ring::new(3);
    for i in 1..=10 {
        ring.push(i);
    }
    let first = ring.since(0);
    assert_eq!(first.missed, 7);
    let second = ring.since(first.next_cursor);
    assert_eq!(second.missed, 0);
    assert!(second.entries.is_empty());
}

// ---------------------------------------------------------------------------
// Capacity and clearing
// ---------------------------------------------------------------------------

#[test]
fn a_zero_capacity_is_raised_to_one_rather_than_keeping_nothing() {
    // A ring that keeps nothing is honest and useless, and a zero is far more
    // likely to be computed than chosen.
    let mut ring = Ring::new(0);
    assert_eq!(ring.capacity(), 1);
    ring.push("a");
    assert_eq!(ring.len(), 1);
}

#[test]
fn clearing_counts_the_cleared_entries_as_dropped() {
    // A navigation discards the previous page's console, and from a reader's
    // point of view that is exactly what eviction is.
    let mut ring = filled(10, 4);
    assert_eq!(ring.dropped(), 0);
    ring.clear();
    assert_eq!(ring.dropped(), 4);
    assert!(ring.is_empty());
}

#[test]
fn clearing_does_not_reset_the_sequence_counter() {
    // The `intents::retire` high-water-mark rule. A reader holding cursor 4 must
    // not be handed a fresh entry numbered 1 and conclude it is caught up.
    let mut ring = filled(10, 4);
    ring.clear();
    assert_eq!(ring.push(99), 5);
    let slice = ring.since(4);
    assert_eq!(slice.entries, vec![&99]);
}

#[test]
fn dropped_is_a_property_of_the_log_and_is_never_reset_by_a_read() {
    let ring = filled(2, 6);
    assert_eq!(ring.dropped(), 4);
    let _ = ring.since(0);
    let _ = ring.since(0);
    assert_eq!(ring.dropped(), 4);
}

// ---------------------------------------------------------------------------
// Cleared is not evicted
// ---------------------------------------------------------------------------

#[test]
fn entries_cleared_with_the_page_are_not_reported_as_buffer_evictions() {
    // The panel is open on a page that logs 40 messages; it closes, reopens and
    // loads a page that logs 2. A reader at cursor 0 was told 40 entries "were
    // evicted from the buffer before this read ... this is not a quiet page, it
    // is a partial record" - a false statement about a page whose record is
    // complete, on the strength of a page the reader never saw. "Cleared
    // because the page it belonged to is gone" and "evicted because the buffer
    // overflowed" are two facts, and only the second licenses distrusting the
    // current record.
    let mut ring: Ring<u32> = Ring::new(500);
    for i in 0..40 {
        ring.push(i);
    }
    ring.clear();
    ring.clear();
    ring.push(100);
    ring.push(101);

    let slice = ring.since(0);
    assert_eq!(slice.entries, vec![&100, &101]);
    assert_eq!(slice.missed, 0, "nothing this page produced was evicted");
    assert_eq!(slice.discarded, 40, "40 belonged to a page that is gone");
}

#[test]
fn eviction_inside_the_current_page_is_still_reported_as_eviction() {
    // The distinction must not become an excuse to stop reporting the real
    // loss: this ring genuinely overflowed after the clear.
    let mut ring: Ring<u32> = Ring::new(5);
    for i in 0..40 {
        ring.push(i);
    }
    ring.clear();
    for i in 100..110 {
        ring.push(i);
    }
    let slice = ring.since(0);
    assert_eq!(slice.entries.len(), 5);
    assert_eq!(slice.discarded, 40, "the previous page's rows");
    assert_eq!(slice.missed, 5, "evicted from this page's own log");
}

#[test]
fn a_reader_that_had_caught_up_before_the_clear_is_charged_nothing() {
    let mut ring: Ring<u32> = Ring::new(500);
    for i in 0..40 {
        ring.push(i);
    }
    ring.clear();
    ring.push(100);
    let slice = ring.since(40);
    assert_eq!(slice.missed, 0);
    assert_eq!(slice.discarded, 0);
}

#[test]
fn a_clear_with_nothing_held_still_moves_the_page_boundary() {
    // `reset_page` runs on open and on close, so a clear over an empty ring is
    // the ordinary case. It must still mark everything before it as belonging
    // to a page that is gone, or the entries evicted *before* the clear come
    // back as this page's evictions.
    let mut ring: Ring<u32> = Ring::new(2);
    for i in 0..6 {
        ring.push(i);
    }
    assert_eq!(ring.since(0).missed, 4);
    ring.clear();
    ring.push(100);
    let slice = ring.since(0);
    assert_eq!(slice.missed, 0);
    assert_eq!(slice.discarded, 6);
}
