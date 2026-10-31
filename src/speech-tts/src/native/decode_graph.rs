//! Pointer-free native exports for safe decode-workspace selection.

use std::ffi::c_int;

// SAFETY: The private static-link ABI passes scalar dimensions only. The
// returned index is checked against the native vector extent before use.
// Invalid input aborts the isolated worker instead of indexing native storage.
#[unsafe(no_mangle)]
pub(super) extern "C" fn bonsai_qt_decode_graph_index(tokens: c_int, windows: usize) -> usize {
    super::super::decode_graph::window_index(tokens, windows)
        .unwrap_or_else(|| std::process::abort())
}

// SAFETY: Scalar context presence and batch widths have no pointer/lifetime
// obligations. The caller releases only completed native graph workspaces.
#[unsafe(no_mangle)]
pub(super) const extern "C" fn bonsai_qt_decode_graph_rebuild(
    has_context: bool,
    cached_width: c_int,
    requested_width: c_int,
) -> bool {
    super::super::decode_graph::rebuild(has_context, cached_width, requested_width)
}
