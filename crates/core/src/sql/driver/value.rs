//! The one-to-one relabelling from what [`crate::sql::format`] renders to what
//! [`crate::sql::model`] puts on the wire.
//!
//! `format::SqlValue` and `model::SqlValue` are the same seven outcomes; the
//! first exists because the rendering rules were written and tested before
//! `model` landed, and its own type comment says so. This conversion is
//! **mechanical**: it decides nothing, invents no variant, and has no fallback
//! arm — every match is exhaustive, so adding an outcome to either enum breaks
//! the build here rather than silently collapsing two answers into one.
//!
//! It lives in `driver/` because the driver is the only caller: it decodes a
//! cell into a [`crate::sql::format::Cell`], lets `format` decide the
//! rendering, and puts the result on the wire.

use crate::sql::format::SqlValue as Rendered;
use crate::sql::model::SqlValue as Wire;

/// Relabel a rendered cell as a wire cell.
pub fn to_wire(value: Rendered) -> Wire {
    match value {
        Rendered::Null => Wire::Null,
        Rendered::Bool { value } => Wire::Bool { value },
        Rendered::Number { text } => Wire::Number { text },
        Rendered::Text { text, truncated } => Wire::Text { text, truncated },
        Rendered::Bytes {
            hex,
            byte_length,
            truncated,
        } => Wire::Bytes {
            hex,
            byte_length,
            truncated,
        },
        Rendered::Unsupported { type_name } => Wire::Unsupported { type_name },
        Rendered::Unavailable { reason } => Wire::Unavailable { reason },
    }
}

#[cfg(test)]
#[path = "value_tests.rs"]
mod tests;
