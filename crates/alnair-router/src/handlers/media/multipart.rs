//! Byte-level helpers for `multipart/form-data` bodies.
//!
//! The body is scanned, never re-encoded: file parts are arbitrary binary, and
//! a parse-and-rebuild would both lose bytes and change lengths.

use axum::body::Bytes;

/// Reads one field out of a `multipart/form-data` body.
///
/// The body is only scanned, never consumed, so the exact bytes the caller sent
/// are still what reaches the upstream. Returns `None` for a non-multipart body
/// or a missing field.
pub(super) fn multipart_field(
    body: &[u8],
    content_type: Option<&str>,
    field: &str,
) -> Option<String> {
    if !multipart_content_type(content_type) {
        return None;
    }

    let range = multipart_value_range(body, field)?;
    let value = String::from_utf8_lossy(&body[range]).trim().to_string();
    (!value.is_empty()).then_some(value)
}

/// Locates the byte range of a multipart field's value.
///
/// Works on raw bytes because file parts are arbitrary binary: a lossy UTF-8
/// view would rewrite its length and shift every offset that follows.
/// `filename="model"` also contains `name="model"`, so an occurrence only
/// counts when it starts a parameter: preceded by `;` or whitespace.
fn multipart_value_range(body: &[u8], field: &str) -> Option<std::ops::Range<usize>> {
    let needle = format!("name=\"{field}\"").into_bytes();

    let mut start = None;
    for index in 0..body.len().saturating_sub(needle.len()) + 1 {
        if &body[index..index + needle.len()] != needle.as_slice() {
            continue;
        }
        let is_parameter_start = index > 0 && matches!(body[index - 1], b';' | b' ' | b'\t');
        if is_parameter_start {
            start = Some(index);
            break;
        }
    }
    let start = start?;

    // The value starts after the blank line closing this part's headers.
    let value_start = find(&body[start..], b"\r\n\r\n")? + start + 4;
    let value_end = find(&body[value_start..], b"\r\n")? + value_start;

    Some(value_start..value_end)
}

/// Index of the first occurrence of `needle` in `haystack`.
fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    (0..=haystack.len() - needle.len())
        .find(|index| &haystack[*index..*index + needle.len()] == needle)
}

/// Replaces one multipart field's value, leaving every other byte alone.
pub(super) fn rewrite_multipart_field(
    body: &[u8],
    content_type: Option<&str>,
    field: &str,
    value: &str,
) -> Bytes {
    let range = multipart_content_type(content_type)
        .then(|| multipart_value_range(body, field))
        .flatten();

    let Some(range) = range else {
        return Bytes::copy_from_slice(body);
    };

    let mut rewritten = Vec::with_capacity(body.len() - range.len() + value.len());
    rewritten.extend_from_slice(&body[..range.start]);
    rewritten.extend_from_slice(value.as_bytes());
    rewritten.extend_from_slice(&body[range.end..]);
    Bytes::from(rewritten)
}

/// True for a `multipart/form-data` content type.
fn multipart_content_type(content_type: Option<&str>) -> bool {
    content_type.is_some_and(|value| {
        value
            .to_ascii_lowercase()
            .starts_with("multipart/form-data")
    })
}
