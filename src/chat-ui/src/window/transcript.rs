//! Read-only native Rich Edit transcript. Selection belongs to the reader;
//! streaming always writes to the document end, independently of that selection.

use super::{ACCENT, MUTED, PANEL, TEXT, control, set_text, wide};
use std::io;
use windows_sys::{
    Win32::{
        Foundation::{FreeLibrary, HMODULE, HWND, POINT},
        System::LibraryLoader::{LOAD_LIBRARY_SEARCH_SYSTEM32, LoadLibraryExW},
        UI::{
            Controls::{EM_REPLACESEL, EM_SCROLLCARET, SetWindowTheme},
            WindowsAndMessaging::{
                ES_AUTOVSCROLL, ES_MULTILINE, ES_NOHIDESEL, ES_READONLY, GetScrollInfo, SB_BOTTOM,
                SB_VERT, SCROLLINFO, SIF_PAGE, SIF_POS, SIF_RANGE, SendMessageW, WM_USER,
                WM_VSCROLL, WS_TABSTOP, WS_VSCROLL,
            },
        },
    },
    core::w,
};

// Rich Edit messages/layouts from the Windows SDK's Richedit.h. windows-sys
// does not expose these definitions. All pointers below are synchronous only.
const EM_EXGETSEL: u32 = WM_USER + 52;
const EM_EXLIMITTEXT: u32 = WM_USER + 53;
const EM_EXSETSEL: u32 = WM_USER + 55;
const EM_SETBKGNDCOLOR: u32 = WM_USER + 67;
const EM_SETCHARFORMAT: u32 = WM_USER + 68;
const EM_SETUNDOLIMIT: u32 = WM_USER + 82;
const EM_GETTEXTLENGTHEX: u32 = WM_USER + 95;
const EM_GETSCROLLPOS: u32 = WM_USER + 221;
const EM_SETSCROLLPOS: u32 = WM_USER + 222;
const SCF_SELECTION: usize = 1;
const CFM_BOLD: u32 = 1;
const CFM_COLOR: u32 = 0x4000_0000;
const CFM_FACE: u32 = 0x2000_0000;
const CFM_SIZE: u32 = 0x8000_0000;

#[repr(C)]
#[derive(Default, Debug, PartialEq, Eq, Clone, Copy)]
struct CharRange {
    start: i32,
    end: i32,
}

#[repr(C)]
struct TextLength {
    flags: u32,
    code_page: u32,
}

#[repr(C)]
#[derive(Default)]
struct CharFormat {
    size: u32,
    mask: u32,
    effects: u32,
    height: i32,
    offset: i32,
    color: u32,
    charset: u8,
    pitch: u8,
    face: [u16; 32],
}

pub(super) struct Library(HMODULE);

impl Library {
    pub(super) fn load() -> io::Result<Self> {
        // SAFETY: Load only the Windows system Rich Edit implementation. The
        // owned module stays loaded until all transcript controls are destroyed.
        let module = unsafe {
            LoadLibraryExW(
                w!("Msftedit.dll"),
                std::ptr::null_mut(),
                LOAD_LIBRARY_SEARCH_SYSTEM32,
            )
        };
        if module.is_null() {
            Err(io::Error::last_os_error())
        } else {
            Ok(Self(module))
        }
    }
}

impl Drop for Library {
    fn drop(&mut self) {
        // SAFETY: This wrapper owns one module reference and outlives its controls.
        unsafe { FreeLibrary(self.0) };
    }
}

pub(super) fn create(parent: HWND, id: usize) -> io::Result<HWND> {
    let style = WS_TABSTOP
        | WS_VSCROLL
        | u32::try_from(ES_MULTILINE | ES_AUTOVSCROLL | ES_READONLY | ES_NOHIDESEL)
            .unwrap_or_default();
    let handle = control(parent, w!("RICHEDIT50W"), "", style, id)?;
    // SAFETY: Live Rich Edit control; scalar limit, no retained pointer.
    unsafe { SendMessageW(handle, EM_EXLIMITTEXT, 0, 4 * 1024 * 1024) };
    // SAFETY: Live control; scalar COLORREF sets its own background color.
    unsafe {
        SendMessageW(
            handle,
            EM_SETBKGNDCOLOR,
            0,
            isize::try_from(PANEL).unwrap_or_default(),
        )
    };
    // SAFETY: Read-only transcript needs no undo storage; scalar message only.
    unsafe { SendMessageW(handle, EM_SETUNDOLIMIT, 0, 0) };
    // SAFETY: Static native theme name is read synchronously by this live control.
    unsafe { SetWindowTheme(handle, w!("DarkMode_Explorer"), std::ptr::null()) };
    Ok(handle)
}

fn length(control: HWND) -> i32 {
    let options = TextLength {
        flags: 2 | 8,    // GTL_PRECISE | GTL_NUMCHARS; internal CR, not exported CRLF.
        code_page: 1200, // Unicode UTF-16, matching CHARRANGE positions.
    };
    // SAFETY: Live control reads this SDK-layout structure synchronously.
    let count =
        unsafe { SendMessageW(control, EM_GETTEXTLENGTHEX, (&raw const options).addr(), 0) };
    i32::try_from(count.max(0)).unwrap_or(i32::MAX)
}

fn selection(control: HWND) -> CharRange {
    let mut range = CharRange::default();
    // SAFETY: Live control writes this SDK-layout range during the call only.
    unsafe {
        SendMessageW(
            control,
            EM_EXGETSEL,
            0,
            (&raw mut range).addr().cast_signed(),
        );
    }
    range
}

fn select(control: HWND, range: CharRange) {
    // SAFETY: Live control reads the SDK-layout range synchronously.
    unsafe {
        SendMessageW(
            control,
            EM_EXSETSEL,
            0,
            (&raw const range).addr().cast_signed(),
        );
    }
}

fn at_bottom(control: HWND) -> bool {
    let mut info = SCROLLINFO {
        cbSize: u32::try_from(size_of::<SCROLLINFO>()).unwrap_or_default(),
        fMask: SIF_RANGE | SIF_PAGE | SIF_POS,
        ..SCROLLINFO::default()
    };
    // SAFETY: Live control writes a correctly sized native scrollbar structure.
    let available = unsafe { GetScrollInfo(control, SB_VERT, &raw mut info) };
    available == 0 || i64::from(info.nPos) + i64::from(info.nPage) >= i64::from(info.nMax)
}

pub(super) fn scroll_to_end(control: HWND) {
    let end = length(control);
    select(control, CharRange { start: end, end });
    // SAFETY: Live control, pointer-free scrolling message.
    unsafe { SendMessageW(control, EM_SCROLLCARET, 0, 0) };
}

pub(super) fn set_font(control: HWND) {
    let mut format = CharFormat {
        size: u32::try_from(size_of::<CharFormat>()).unwrap_or_default(),
        mask: CFM_FACE | CFM_SIZE,
        height: 255, // 12.75 points in twips; 17 pixels at 96 DPI.
        ..CharFormat::default()
    };
    for (target, source) in format.face.iter_mut().zip("Segoe UI".encode_utf16()) {
        *target = source;
    }
    // SAFETY: SCF_ALL updates only font face/size, retaining speaker emphasis
    // and colors through DPI changes. The control reads the structure inline.
    unsafe {
        SendMessageW(
            control,
            EM_SETCHARFORMAT,
            4,
            (&raw const format).addr().cast_signed(),
        );
    }
}

pub(super) fn append(control: HWND, text: &str, color: u32, bold: bool) {
    if text.is_empty() {
        return;
    }
    let saved = selection(control);
    let end = length(control);
    let follow = saved.start == saved.end && at_bottom(control);
    let mut scroll = POINT::default();
    // SAFETY: Live control writes a message-scoped native POINT.
    unsafe {
        SendMessageW(
            control,
            EM_GETSCROLLPOS,
            0,
            (&raw mut scroll).addr().cast_signed(),
        );
    }
    select(control, CharRange { start: end, end });
    let format = CharFormat {
        size: u32::try_from(size_of::<CharFormat>()).unwrap_or_default(),
        mask: CFM_BOLD | CFM_COLOR,
        effects: u32::from(bold),
        color,
        ..CharFormat::default()
    };
    // SAFETY: The SDK-layout format is live until this synchronous call returns.
    unsafe {
        SendMessageW(
            control,
            EM_SETCHARFORMAT,
            SCF_SELECTION,
            (&raw const format).addr().cast_signed(),
        );
    }
    // Rich Edit stores paragraph breaks as CR. Normalize composer CRLF too,
    // rather than adding an extra carriage return to each existing CRLF.
    let text = wide(&text.replace("\r\n", "\n").replace('\n', "\r"));
    // SAFETY: The native control copies this NUL-terminated buffer synchronously.
    unsafe {
        SendMessageW(
            control,
            EM_REPLACESEL,
            0,
            text.as_ptr().addr().cast_signed(),
        )
    };
    if follow && saved.start == end {
        scroll_to_end(control);
    } else {
        select(control, saved);
        if follow {
            // SAFETY: Follow the bottom without moving the reader's caret.
            // Scrolling back down resumes following after reading earlier text.
            unsafe {
                SendMessageW(
                    control,
                    WM_VSCROLL,
                    usize::try_from(SB_BOTTOM).unwrap_or_default(),
                    0,
                );
            }
        } else {
            // SAFETY: Restore the reader's viewport using a live native POINT.
            unsafe {
                SendMessageW(
                    control,
                    EM_SETSCROLLPOS,
                    0,
                    (&raw const scroll).addr().cast_signed(),
                );
            }
        }
    }
}

pub(super) fn turn(control: HWND, speaker: &str, color: u32) {
    if length(control) != 0 {
        append(control, "\n\n", TEXT, false);
    }
    append(control, &format!("{speaker}\n"), color, true);
}

pub(super) fn welcome(control: HWND) {
    set_text(control, "");
    turn(control, "What would you like to create?", ACCENT);
    append(
        control,
        "Write a message below to start a conversation.\n\n",
        TEXT,
        false,
    );
    append(
        control,
        "Enter to send · Shift+Enter for a new line\nYour conversation stays in this window's memory.",
        MUTED,
        false,
    );
}

#[cfg(test)]
mod tests;
