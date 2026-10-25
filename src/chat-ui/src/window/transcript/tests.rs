//! Focused real-control coverage for streaming independently of the reader.

use super::*;
use std::ptr::{null, null_mut};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DestroyWindow, MoveWindow, SB_PAGEUP, SB_TOP, WS_OVERLAPPEDWINDOW,
};

const EM_GETCHARFORMAT: u32 = WM_USER + 58;
const EM_GETTEXTRANGE: u32 = WM_USER + 75;

#[repr(C)]
struct TextRange {
    range: CharRange,
    text: *mut u16,
}

struct TestWindow(HWND);

impl TestWindow {
    fn create() -> io::Result<Self> {
        // SAFETY: STATIC is a system class; all pointers are static or null.
        // The hidden top-level test window is owned by this test thread.
        let window = unsafe {
            CreateWindowExW(
                0,
                w!("STATIC"),
                w!("Transcript regression"),
                WS_OVERLAPPEDWINDOW,
                0,
                0,
                420,
                260,
                null_mut(),
                null_mut(),
                null_mut(),
                null(),
            )
        };
        if window.is_null() {
            Err(io::Error::last_os_error())
        } else {
            Ok(Self(window))
        }
    }
}

impl Drop for TestWindow {
    fn drop(&mut self) {
        // SAFETY: The test owns this live thread-local parent and its children.
        unsafe { DestroyWindow(self.0) };
    }
}

fn document(control: HWND) -> String {
    let count = length(control);
    let mut text = vec![0u16; usize::try_from(count).unwrap_or_default() + 1];
    let range = TextRange {
        range: CharRange {
            start: 0,
            end: count,
        },
        text: text.as_mut_ptr(),
    };
    // SAFETY: Rich Edit reads this SDK-layout range synchronously and writes at
    // most count UTF-16 units plus a terminator to the supplied live allocation.
    let copied = unsafe {
        SendMessageW(
            control,
            EM_GETTEXTRANGE,
            0,
            (&raw const range).addr().cast_signed(),
        )
    };
    String::from_utf16_lossy(&text[..usize::try_from(copied.max(0)).unwrap_or_default()])
}

fn format_at(control: HWND, position: i32) -> CharFormat {
    select(
        control,
        CharRange {
            start: position,
            end: position + 1,
        },
    );
    let mut format = CharFormat {
        size: u32::try_from(size_of::<CharFormat>()).unwrap_or_default(),
        ..CharFormat::default()
    };
    // SAFETY: Live Rich Edit writes to the correctly sized SDK-layout structure
    // during this synchronous query of the selected character's formatting.
    unsafe {
        SendMessageW(
            control,
            EM_GETCHARFORMAT,
            SCF_SELECTION,
            (&raw mut format).addr().cast_signed(),
        );
    }
    format
}

fn scroll_position(control: HWND) -> POINT {
    let mut point = POINT::default();
    // SAFETY: Live Rich Edit writes this message-scoped native POINT.
    unsafe {
        SendMessageW(
            control,
            EM_GETSCROLLPOS,
            0,
            (&raw mut point).addr().cast_signed(),
        );
    }
    point
}

#[test]
#[allow(
    clippy::too_many_lines,
    reason = "One real-control scenario verifies coordinated selection, formatting and scroll behavior"
)]
fn native_transcript_streaming_preserves_reader_state() -> io::Result<()> {
    // The module remains loaded until the parent and all child controls drop.
    let _library = Library::load()?;
    let parent = TestWindow::create()?;
    let transcript = create(parent.0, 201)?;
    // SAFETY: The test owns this live child; scalar dimensions force a small
    // viewport so this fixture exercises real wrapping and vertical scrolling.
    unsafe { MoveWindow(transcript, 0, 0, 360, 160, 1) };

    turn(transcript, "You", ACCENT);
    let body = format!("{}🌳\r\nlast", "Earlier text.\r\n".repeat(6000));
    append(transcript, &body, TEXT, false);
    let mut expected = format!("You\r{}", body.replace("\r\n", "\r"));
    assert!(length(transcript) > 65_535);
    assert_eq!(document(transcript), expected);

    let caret = CharRange { start: 7, end: 7 };
    select(transcript, caret);
    append(transcript, " first delta", TEXT, false);
    expected.push_str(" first delta");
    assert_eq!(selection(transcript), caret);
    assert_eq!(document(transcript), expected);

    let selected = CharRange {
        start: 65_540,
        end: 65_560,
    };
    select(transcript, selected);
    append(transcript, " second 🌱\r\nline", TEXT, false);
    expected.push_str(" second 🌱\rline");
    assert_eq!(selection(transcript), selected);
    assert_eq!(document(transcript), expected);

    set_font(transcript);
    let role = format_at(transcript, 0);
    assert_ne!(role.mask & CFM_BOLD, 0);
    assert_ne!(role.effects & CFM_BOLD, 0);
    assert_eq!(role.color, ACCENT);
    let body_format = format_at(transcript, 4);
    assert_ne!(body_format.mask & CFM_BOLD, 0);
    assert_eq!(body_format.effects & CFM_BOLD, 0);
    assert_eq!(body_format.color, TEXT);
    let delta_format = format_at(transcript, length(transcript) - 2);
    assert_eq!(delta_format.effects & CFM_BOLD, 0);
    assert_eq!(delta_format.color, TEXT);

    scroll_to_end(transcript);
    let previous_end = length(transcript);
    assert!(at_bottom(transcript));
    append(
        transcript,
        "\nA new answer line.\nAnother answer line.",
        TEXT,
        false,
    );
    expected.push_str("\rA new answer line.\rAnother answer line.");
    let new_end = length(transcript);
    assert!(new_end > previous_end);
    assert_eq!(
        selection(transcript),
        CharRange {
            start: new_end,
            end: new_end
        }
    );
    assert!(at_bottom(transcript));
    assert!(scroll_position(transcript).y > 0);

    // SAFETY: A scalar native scroll message reproduces scrolling upward while
    // the insertion selection still sits at the document end.
    unsafe {
        SendMessageW(
            transcript,
            WM_VSCROLL,
            usize::try_from(SB_TOP).unwrap_or_default(),
            0,
        )
    };
    assert!(!at_bottom(transcript));
    let reader_scroll = scroll_position(transcript);
    let reader_selection = selection(transcript);
    append(
        transcript,
        "\nText arriving while reading above.",
        TEXT,
        false,
    );
    expected.push_str("\rText arriving while reading above.");
    let restored_scroll = scroll_position(transcript);
    assert_eq!(
        (restored_scroll.x, restored_scroll.y),
        (reader_scroll.x, reader_scroll.y)
    );
    assert_eq!(selection(transcript), reader_selection);
    assert_eq!(document(transcript), expected);

    scroll_to_end(transcript);
    // SAFETY: Scalar message simulates reading one page above the live answer.
    unsafe {
        SendMessageW(
            transcript,
            WM_VSCROLL,
            usize::try_from(SB_PAGEUP).unwrap_or_default(),
            0,
        )
    };
    let mut before = SCROLLINFO {
        cbSize: u32::try_from(size_of::<SCROLLINFO>()).unwrap_or_default(),
        fMask: SIF_POS,
        ..SCROLLINFO::default()
    };
    // SAFETY: This live control writes a correctly sized scrollbar structure.
    unsafe { GetScrollInfo(transcript, SB_VERT, &raw mut before) };
    append(transcript, "\nAnother streamed line.", TEXT, false);
    let mut after = SCROLLINFO {
        cbSize: u32::try_from(size_of::<SCROLLINFO>()).unwrap_or_default(),
        fMask: SIF_POS,
        ..SCROLLINFO::default()
    };
    // SAFETY: This live control writes a correctly sized scrollbar structure.
    unsafe { GetScrollInfo(transcript, SB_VERT, &raw mut after) };
    assert_eq!(after.nPos, before.nPos);

    // SAFETY: Scalar scroll input returns to the latest text without changing
    // the reader's old caret after deltas arrived while scrolled away.
    unsafe {
        SendMessageW(
            transcript,
            WM_VSCROLL,
            usize::try_from(SB_BOTTOM).unwrap_or_default(),
            0,
        )
    };
    let resumed_caret = selection(transcript);
    assert!(at_bottom(transcript));
    append(
        transcript,
        "\nFollowing resumes after scrolling down.\nNew line.",
        TEXT,
        false,
    );
    assert!(at_bottom(transcript));
    assert_eq!(selection(transcript), resumed_caret);

    welcome(transcript);
    assert!(document(transcript).starts_with("What would you like to create?\r"));
    set_text(transcript, "");
    turn(transcript, "You", ACCENT);
    append(transcript, "A fresh conversation.", TEXT, false);
    assert_eq!(document(transcript), "You\rA fresh conversation.");
    let fresh_role = format_at(transcript, 0);
    assert_ne!(fresh_role.effects & CFM_BOLD, 0);
    assert_eq!(fresh_role.color, ACCENT);
    let fresh_body = format_at(transcript, 4);
    assert_eq!(fresh_body.effects & CFM_BOLD, 0);
    assert_eq!(fresh_body.color, TEXT);
    Ok(())
}
