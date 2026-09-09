//! A bounded log that says what it lost.
//!
//! The console and network logs of a live page are unbounded — a polling SPA
//! emits a request every few seconds forever — so they have to be capped. The
//! interesting part is not the cap but the two ways a capped log lies:
//!
//! 1. **It drops entries and does not say so.** A reader sees ten rows and has
//!    no way to know four hundred came before them, so "no errors in the
//!    console" is asserted from a window that never contained them.
//! 2. **A cursor silently skips the gap.** A reader that asked for everything
//!    after sequence 12, in a ring whose oldest surviving entry is 400, is
//!    handed rows 400 onwards as though 13..399 had never existed. That is the
//!    worse failure: the reader *believes* it has a complete tail.
//!
//! So [`Ring::push`] counts drops and [`Ring::since`] reports the gap in the
//! same value as the rows. A caller cannot read the rows without being handed
//! the number of entries it missed.
//!
//! Sequence numbers are monotonic from 1 and are never reused. They are the
//! cursor, not an index into the buffer, precisely so that eviction cannot
//! renumber anything.

use std::collections::VecDeque;

/// The default cap for one page's console or network log.
///
/// Sized for reading, not for archiving: this is a live debugging surface, and
/// a reader that needs more than this needs the page's own devtools. Every drop
/// is counted, so the cap costs information the reader is told about rather than
/// information it silently lacks.
pub const DEFAULT_CAPACITY: usize = 500;

/// What a reader missed, alongside what it got.
///
/// The gap and the rows travel together on purpose: there is no way to read the
/// entries without also receiving [`Self::missed`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Slice<'a, T> {
    /// The surviving entries with a sequence greater than the requested cursor,
    /// oldest first.
    pub entries: Vec<&'a T>,
    /// How many entries after the requested cursor were evicted **from the
    /// current page's own log** before this read, and so are not in
    /// [`Self::entries`] and never will be.
    ///
    /// Zero means the tail is genuinely complete from the cursor. Non-zero is
    /// the fact a reader must not be allowed to miss: this page produced rows
    /// the buffer threw away, so what follows is a partial record of it.
    ///
    /// Deliberately **not** the same number as [`Self::discarded`]; see there.
    pub missed: u64,
    /// How many entries after the requested cursor belonged to a page that is
    /// gone, and were discarded with it by [`Ring::clear`].
    ///
    /// A separate fact from [`Self::missed`], and only `missed` licenses
    /// distrusting the *current* record. Folded together, a reader that opened
    /// the panel on a chatty page, closed it and loaded a quiet one was told
    /// forty entries had been evicted and that "this is not a quiet page, it is
    /// a partial record" - a false statement about the page on screen, made on
    /// the strength of a page the reader never saw.
    pub discarded: u64,
    /// The cursor to pass next time. Advances past everything returned, and
    /// past the gap, so a reader that is behind does not re-report the same gap
    /// forever.
    pub next_cursor: u64,
}

/// A bounded, drop-counting, gap-reporting ring.
#[derive(Debug, Clone)]
pub struct Ring<T> {
    /// `(seq, item)` oldest first.
    entries: VecDeque<(u64, T)>,
    capacity: usize,
    /// The sequence the next push will take. Starts at 1, so 0 is always a
    /// usable "I have seen nothing" cursor.
    next_seq: u64,
    dropped: u64,
    /// The highest sequence [`Ring::clear`] has discarded. Everything at or
    /// below it belonged to a page that is gone, which is what lets
    /// [`Slice::discarded`] be a different number from [`Slice::missed`].
    cleared_through: u64,
}

impl<T> Ring<T> {
    /// A ring holding at most `capacity` entries.
    ///
    /// A zero capacity is raised to one rather than accepted. A ring that keeps
    /// nothing would report every entry as dropped, which is *technically*
    /// honest and useless, and it is far more likely to be a computed zero than
    /// a deliberate choice.
    pub fn new(capacity: usize) -> Self {
        Self {
            entries: VecDeque::new(),
            capacity: capacity.max(1),
            next_seq: 1,
            dropped: 0,
            cleared_through: 0,
        }
    }

    /// Append an entry, evicting the oldest if the ring is full.
    ///
    /// Returns the sequence number it was given.
    pub fn push(&mut self, item: T) -> u64 {
        let seq = self.next_seq;
        self.next_seq += 1;
        self.entries.push_back((seq, item));
        while self.entries.len() > self.capacity {
            self.entries.pop_front();
            self.dropped += 1;
        }
        seq
    }

    /// How many entries have been evicted over this ring's whole life.
    ///
    /// Never reset by a read: it is a property of the log, not of a reader's
    /// position. A reader's own miss count is [`Slice::missed`].
    pub fn dropped(&self) -> u64 {
        self.dropped
    }

    /// How many entries are currently held.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// The sequence of the newest entry, or 0 when nothing has ever been pushed.
    ///
    /// A cursor equal to this means "caught up".
    pub fn latest_seq(&self) -> u64 {
        self.next_seq - 1
    }

    /// Everything after `cursor`, plus how much of it is gone.
    ///
    /// A `cursor` of 0 means "I have seen nothing", and a ring that has already
    /// evicted entries will therefore report a non-zero [`Slice::missed`] — a
    /// first read is not exempt, because a reader attaching to a page that has
    /// been open for an hour has missed the interesting part.
    ///
    /// A cursor **ahead** of the ring (a reader that survived a page reload, or
    /// a fabricated value) returns nothing and no gap: the entries it names do
    /// not exist yet, which is not a loss.
    pub fn since(&self, cursor: u64) -> Slice<'_, T> {
        let entries: Vec<&T> = self
            .entries
            .iter()
            .filter(|(seq, _)| *seq > cursor)
            .map(|(_, item)| item)
            .collect();

        // The oldest sequence still held. When the ring is empty there is no
        // surviving entry, so the boundary is "everything ever pushed".
        let oldest_held = self.entries.front().map_or(self.next_seq, |(seq, _)| *seq);
        // Entries the reader asked for (seq > cursor).
        let wanted_from = cursor + 1;
        // The first sequence belonging to the page currently being logged.
        let page_from = self.cleared_through + 1;
        // Split, never summed: what went with a previous page, and what this
        // page's own log lost to the cap. Both are counted from the cursor, so
        // a reader that had caught up before the clear is charged nothing.
        let discarded = page_from.saturating_sub(wanted_from);
        let missed = oldest_held.saturating_sub(wanted_from.max(page_from));

        Slice {
            entries,
            missed,
            discarded,
            // Never moves backwards: a cursor ahead of the ring is left alone,
            // so a stale reader is not silently rewound into re-reading.
            next_cursor: self.latest_seq().max(cursor),
        }
    }

    /// Forget everything, keeping the sequence counter.
    ///
    /// Used when the page changes: the previous page's console is not this
    /// page's. The counter is **not** reset, for the same reason
    /// `intents::retire` must never lower its high-water mark — a reader holding
    /// cursor 400 must not be handed a fresh entry numbered 1 and conclude it is
    /// already caught up. Cleared entries are counted as dropped, because from a
    /// reader's point of view that is exactly what happened to them.
    pub fn clear(&mut self) {
        self.dropped += self.entries.len() as u64;
        // Where the page boundary fell. This is what lets a later reader be
        // told separately how many entries went with a page that is gone and
        // how many this page's own log lost to the cap; merged, the first was
        // reported as the second and a complete record read as a partial one.
        self.cleared_through = self.latest_seq();
        self.entries.clear();
    }
}

impl<T> Default for Ring<T> {
    fn default() -> Self {
        Self::new(DEFAULT_CAPACITY)
    }
}

#[cfg(test)]
#[path = "ring_tests.rs"]
mod tests;
