//! Cutting a page's text down to a readable size, and admitting it.
//!
//! A rendered page's text is unbounded — a log viewer or an infinite-scroll feed
//! is megabytes — and an agent's context is not, so it has to be capped. The
//! decision worth a module is what the cap *reports*.
//!
//! **The total is the page's, never the slice's.** This is the same rule as
//! [`crate::lsp::model::UsageResult::total`], and for the same reason: the
//! number is the claim the reader acts on. An agent handed 20,000 characters and
//! told the total is 20,000 concludes it has read the page and that the string it
//! is looking for is not on it. Told the total is 400,000, it knows to search a
//! different way. A cap that also caps the number turns a bounded read into a
//! wrong answer.

use super::model::PageText;

/// The default ceiling on returned page text, in `char`s.
///
/// Chosen to be comfortably readable rather than complete. It is not a tuning
/// knob for completeness — completeness is what [`PageText::truncated`] and
/// [`PageText::total_chars`] are for.
pub const DEFAULT_TEXT_LIMIT: usize = 40_000;

/// Cap `text` at `limit` characters, reporting the real total.
///
/// Counts and cuts in `char`s, not bytes, so the returned string is always valid
/// UTF-8 and a multi-byte character is never split — a page in Japanese would
/// otherwise be cut mid-codepoint at a byte boundary. (`char` boundaries are not
/// grapheme boundaries, so an emoji with a modifier can still lose its
/// modifier; that is a cosmetic loss in a text dump, and pulling in a grapheme
/// segmenter for it would be the only dependency in this module.)
pub fn truncate_page_text(text: &str, limit: usize) -> PageText {
    let total_chars = text.chars().count();
    if total_chars <= limit {
        return PageText {
            text: text.to_owned(),
            total_chars,
            returned_chars: total_chars,
            truncated: false,
        };
    }
    let kept: String = text.chars().take(limit).collect();
    PageText {
        // `limit` may be 0, in which case this is empty — and `truncated` is
        // still true, which is the whole point: an empty answer that does not
        // say it was cut is indistinguishable from an empty page.
        returned_chars: kept.chars().count(),
        text: kept,
        total_chars,
        truncated: true,
    }
}

#[cfg(test)]
#[path = "text_tests.rs"]
mod tests;
