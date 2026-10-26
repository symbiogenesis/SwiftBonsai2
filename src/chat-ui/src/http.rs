//! Shared media-type subset for the local JSON and event-stream adapters.

#![forbid(unsafe_code)]
#![allow(
    clippy::redundant_pub_crate,
    reason = "The native HTTP adapters share this crate-private parser."
)]

/// Accept the expected media type with at most one explicit UTF-8 charset.
pub(super) fn utf8_media_type(value: &[u8], expected: &str) -> bool {
    let Ok(value) = std::str::from_utf8(value) else {
        return false;
    };
    let mut parts = value.split(';');
    if !parts
        .next()
        .is_some_and(|media| media.trim().eq_ignore_ascii_case(expected))
    {
        return false;
    }
    let Some(parameter) = parts.next() else {
        return true;
    };
    let Some((name, charset)) = parameter.split_once('=') else {
        return false;
    };
    name.trim().eq_ignore_ascii_case("charset")
        && (charset.trim().eq_ignore_ascii_case("utf-8")
            || charset.trim().eq_ignore_ascii_case("\"utf-8\""))
        && parts.next().is_none()
}

#[cfg(test)]
mod tests {
    use super::utf8_media_type;

    #[test]
    fn local_media_types_share_the_same_utf8_parameter_subset() {
        for media in ["application/json", "text/event-stream"] {
            for suffix in ["", "; CHARSET=UTF-8", " ; charset = \"utf-8\""] {
                assert!(utf8_media_type(
                    format!("{media}{suffix}").as_bytes(),
                    media
                ));
            }
            for suffix in [
                "-extra",
                ";",
                "; charset=latin1",
                "; charset=utf-8; charset=utf-8",
                "; charset=\"utf-8",
                "; other=utf-8",
            ] {
                assert!(!utf8_media_type(
                    format!("{media}{suffix}").as_bytes(),
                    media
                ));
            }
            assert!(!utf8_media_type(&[0xff], media));
        }
        assert!(!utf8_media_type(b"application/json", "text/event-stream"));
        assert!(!utf8_media_type(b"text/event-stream", "application/json"));
    }
}
