//! Exposed Jinja content capabilities from pinned production marker probes.

use crate::Result;

/// Raw observations from one production rendering with statistics enabled.
pub(super) struct Observation {
    /// Rendered bytes, including any non-UTF-8 bytes accepted by the runtime.
    pub(super) rendered: Vec<u8>,
    /// Whether rendering completed without a runtime exception.
    pub(super) success: bool,
    /// Whether `messages[0].content` recorded `selectattr` or `array_access`.
    pub(super) content_array_access: bool,
}

/// Returns string and typed-content support using the exact upstream probe.
/// Other upstream capability fields are not exposed by this API.
pub(super) fn detect(mut render: impl FnMut(&str) -> Result<Observation>) -> Result<(bool, bool)> {
    const VARIABLES: &str = r#"{"messages":[{"role":"user","content":"STRING_MARKER"}],"tools":[],"bos_token":"","eos_token":"","add_generation_prompt":true}"#;
    let observation = render(VARIABLES)?;
    let typed = observation.content_array_access;
    let marker = observation
        .rendered
        .windows(b"STRING_MARKER".len())
        .any(|bytes| bytes == b"STRING_MARKER");
    let string = observation.success && (!typed || marker);
    Ok((string, typed))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_probe_distinguishes_iteration_from_rendered_marker() -> Result<()> {
        for (rendered, success, array, expected) in [
            (b"STRING_MARKER".as_slice(), true, false, (true, false)),
            (b"STRING_MARKER".as_slice(), true, true, (true, true)),
            (b"".as_slice(), true, true, (false, true)),
            (b"".as_slice(), false, false, (false, false)),
        ] {
            assert_eq!(
                detect(|variables| {
                    assert!(variables.contains("STRING_MARKER"));
                    Ok(Observation {
                        rendered: rendered.to_vec(),
                        success,
                        content_array_access: array,
                    })
                })?,
                expected
            );
        }
        Ok(())
    }
}
