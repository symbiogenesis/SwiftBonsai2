//! Win32 controls and message dispatch, confined to the main UI thread.
//!
//! Control handles never cross threads. Reentrant messages use `try_borrow_mut`
//! rather than producing overlapping references to the window state.
#![allow(
    clippy::redundant_pub_crate,
    reason = "Keep the audited native boundary explicitly parent-only."
)]

use crate::client::{self, Command, Config, Event};
mod transcript;
use std::{
    cell::RefCell,
    io,
    ptr::{null, null_mut, without_provenance_mut},
    sync::mpsc::{self, Receiver, Sender},
};
use windows_sys::Win32::UI::WindowsAndMessaging::WM_GETMINMAXINFO;
use windows_sys::{
    Win32::{
        Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM},
        Graphics::Dwm::DwmSetWindowAttribute,
        Graphics::Gdi::{
            BeginPaint, CLEARTYPE_QUALITY, CLIP_DEFAULT_PRECIS, CreateFontW, CreateSolidBrush,
            DC_PEN, DEFAULT_CHARSET, DEFAULT_PITCH, DT_CENTER, DT_NOPREFIX, DT_SINGLELINE,
            DT_VCENTER, DeleteObject, DrawFocusRect, DrawTextW, EndPaint, FW_NORMAL, FillRect,
            GetStockObject, HBRUSH, HDC, HFONT, InvalidateRect, OUT_DEFAULT_PRECIS, PAINTSTRUCT,
            RoundRect, SelectObject, SetBkColor, SetBkMode, SetDCPenColor, SetTextColor,
            TRANSPARENT,
        },
        System::LibraryLoader::GetModuleHandleW,
        UI::{
            Controls::{
                DRAWITEMSTRUCT, EM_SETLIMITTEXT, ODS_DISABLED, ODS_FOCUS, ODS_SELECTED, ODT_BUTTON,
                SetWindowTheme,
            },
            HiDpi::{
                DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, GetDpiForWindow,
                SetProcessDpiAwarenessContext,
            },
            Input::KeyboardAndMouse::{
                EnableWindow, GetFocus, GetKeyState, SetFocus, VK_RETURN, VK_SHIFT,
            },
            WindowsAndMessaging::{
                BS_OWNERDRAW, CW_USEDEFAULT, CreateWindowExW, DefWindowProcW, DestroyWindow,
                DispatchMessageW, ES_AUTOVSCROLL, ES_MULTILINE, ES_READONLY, ES_WANTRETURN,
                GetClientRect, GetDlgCtrlID, GetMessageW, GetWindowTextLengthW, GetWindowTextW,
                IDC_ARROW, IDI_APPLICATION, IsDialogMessageW, IsWindow, KillTimer, LoadCursorW,
                LoadIconW, MB_ICONERROR, MB_OK, MINMAXINFO, MSG, MessageBoxW, MoveWindow,
                PostQuitMessage, RegisterClassW, SW_HIDE, SW_SHOW, SendMessageW, SetTimer,
                SetWindowTextW, ShowWindow, TranslateMessage, WM_CLOSE, WM_COMMAND, WM_CTLCOLORBTN,
                WM_CTLCOLOREDIT, WM_CTLCOLORSTATIC, WM_DESTROY, WM_DPICHANGED, WM_DRAWITEM,
                WM_ERASEBKGND, WM_GETFONT, WM_KEYDOWN, WM_PAINT, WM_SETFONT, WM_SIZE, WM_TIMER,
                WNDCLASSW, WS_CHILD, WS_CLIPCHILDREN, WS_EX_CONTROLPARENT, WS_OVERLAPPEDWINDOW,
                WS_TABSTOP, WS_VISIBLE, WS_VSCROLL,
            },
        },
    },
    core::w,
};

const SEND: usize = 101;
const STOP: usize = 102;
const NEW: usize = 103;
const THINKING: usize = 104;
const RETRY: usize = 105;
const TIMER: usize = 1;
const BACKGROUND: u32 = 0x0016_1810;
const PANEL: u32 = 0x001d_2217;
const BORDER: u32 = 0x0033_3d2b;
const TEXT: u32 = 0x00df_e8e2;
const MUTED: u32 = 0x009f_a99a;
const ACCENT: u32 = 0x009d_b583;
const BUTTON: u32 = 0x0024_2c1d;

thread_local! {
    static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
    static PALETTE: RefCell<Option<Palette>> = const { RefCell::new(None) };
}

#[allow(
    clippy::struct_excessive_bools,
    reason = "Readiness, generation, retry eligibility and optional pane visibility are independent UI state flags"
)]
struct State {
    window: HWND,
    title: HWND,
    subtitle: HWND,
    transcript: HWND,
    reasoning: HWND,
    input: HWND,
    send: HWND,
    stop: HWND,
    new_chat: HWND,
    thinking: HWND,
    retry: HWND,
    status: HWND,
    font: HFONT,
    title_font: HFONT,
    small_font: HFONT,
    commands: Sender<Command>,
    events: Receiver<Event>,
    ready: bool,
    busy: bool,
    retryable: bool,
    show_thinking: bool,
    chat_started: bool,
}

struct Brush(HBRUSH);
impl Brush {
    fn new(color: u32) -> io::Result<Self> {
        // SAFETY: Scalar COLORREF creates a new owned GDI brush.
        let brush = unsafe { CreateSolidBrush(color) };
        if brush.is_null() {
            Err(io::Error::last_os_error())
        } else {
            Ok(Self(brush))
        }
    }
}
impl Drop for Brush {
    fn drop(&mut self) {
        // SAFETY: Owned brushes are deselected after painting and released only
        // after child controls are destroyed. Each wrapper releases its handle once.
        unsafe {
            DeleteObject(self.0);
        }
    }
}
struct Palette {
    background: Brush,
    panel: Brush,
    button: Brush,
    accent: Brush,
}
impl Palette {
    fn new() -> io::Result<Self> {
        Ok(Self {
            background: Brush::new(BACKGROUND)?,
            panel: Brush::new(PANEL)?,
            button: Brush::new(BUTTON)?,
            accent: Brush::new(ACCENT)?,
        })
    }
}
fn palette_with<T>(action: impl FnOnce(&Palette) -> T) -> Option<T> {
    PALETTE.with(|palette| palette.try_borrow().ok()?.as_ref().map(action))
}
fn color_control(wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let dc: HDC = without_provenance_mut(wparam);
    let control: HWND = without_provenance_mut(lparam.cast_unsigned());
    // SAFETY: WM_CTLCOLOR supplies a live control and message-scoped paint HDC.
    let id = unsafe { GetDlgCtrlID(control) };
    let editor = matches!(id, 201..=203);
    let foreground = if id == 202 || (!editor && id != 301) {
        MUTED
    } else {
        TEXT
    };
    // SAFETY: dc is the live paint context supplied by this synchronous message.
    unsafe {
        SetTextColor(dc, foreground);
    }
    // SAFETY: Same message-scoped HDC; only scalar colors are passed.
    unsafe {
        SetBkColor(dc, if editor { PANEL } else { BACKGROUND });
    }
    palette_with(|p| if editor { p.panel.0 } else { p.background.0 })
        .map_or(0, |brush| brush.addr().cast_signed())
}
fn round_box(dc: HDC, rect: RECT, brush: HBRUSH, border: u32, radius: i32) {
    // SAFETY: Paint DC and palette brush are live; selections are restored below.
    let old_brush = unsafe { SelectObject(dc, brush) };
    // SAFETY: DC_PEN is a borrowed system stock object and is never deleted.
    let pen = unsafe { GetStockObject(DC_PEN) };
    // SAFETY: Live paint DC and stock pen; restore previous selection below.
    let old_pen = unsafe { SelectObject(dc, pen) };
    // SAFETY: Changes only this paint DC's stock pen color.
    unsafe {
        SetDCPenColor(dc, border);
    }
    // SAFETY: Bounded layout coordinates are drawn into a live clipped paint DC.
    unsafe {
        RoundRect(
            dc,
            rect.left,
            rect.top,
            rect.right,
            rect.bottom,
            radius,
            radius,
        );
    }
    // SAFETY: Restore the previous brush before releasing the palette borrow.
    unsafe {
        SelectObject(dc, old_brush);
    }
    // SAFETY: Restore the previous borrowed pen; no ownership is transferred.
    unsafe {
        SelectObject(dc, old_pen);
    }
}
fn paint(window: HWND) {
    let mut scope = PAINTSTRUCT::default();
    // SAFETY: WM_PAINT supplies a live HWND; scope is writable and paired with EndPaint below.
    let dc = unsafe { BeginPaint(window, &raw mut scope) };
    let mut bounds = RECT::default();
    // SAFETY: Live UI-thread HWND and writable rectangle.
    unsafe {
        GetClientRect(window, &raw mut bounds);
    }
    // SAFETY: Live HWND queried only for scalar DPI.
    let dpi = i32::try_from(unsafe { GetDpiForWindow(window) }.max(96)).unwrap_or(96);
    let px = |v: i32| v.saturating_mul(dpi) / 96;
    let pad = px(32);
    let thinking = STATE
        .with(|state| {
            state
                .try_borrow()
                .ok()
                .and_then(|state| state.as_ref().map(|state| state.show_thinking))
        })
        .unwrap_or(false);
    let top = px(148);
    let bottom = bounds.bottom - px(184);
    let split = if thinking { (bottom - top) / 3 } else { 0 };
    palette_with(|p| {
        // SAFETY: Brush and paint DC are live during this synchronous paint scope.
        unsafe {
            FillRect(dc, &raw const bounds, p.background.0);
        }
        let body = RECT {
            left: pad,
            top,
            right: bounds.right - pad,
            bottom: bottom - split,
        };
        round_box(dc, body, p.panel.0, BORDER, px(16));
        if thinking {
            let reasoning = RECT {
                left: pad,
                top: bottom - split + px(10),
                right: bounds.right - pad,
                bottom,
            };
            round_box(dc, reasoning, p.panel.0, BORDER, px(16));
        }
        let composer = RECT {
            left: pad,
            top: bottom + px(16),
            right: bounds.right - pad,
            bottom: bounds.bottom - px(49),
        };
        round_box(dc, composer, p.panel.0, BORDER, px(16));
    });
    // SAFETY: Completes the exact BeginPaint scope for this window.
    unsafe {
        EndPaint(window, &raw const scope);
    }
}
fn draw_button(draw: &DRAWITEMSTRUCT) {
    if draw.CtlType != ODT_BUTTON {
        return;
    }
    palette_with(|p| {
        let primary = matches!(usize::try_from(draw.CtlID), Ok(SEND | RETRY));
        let disabled = draw.itemState & ODS_DISABLED != 0;
        let selected = draw.itemState & ODS_SELECTED != 0;
        let brush = if primary && !disabled && !selected {
            p.accent.0
        } else {
            p.button.0
        };
        round_box(
            draw.hDC,
            draw.rcItem,
            brush,
            if primary && !disabled { ACCENT } else { BORDER },
            12,
        );
        let color = if disabled {
            0x0060_6b58
        } else if primary && !selected {
            BACKGROUND
        } else {
            TEXT
        };
        // SAFETY: DRAWITEMSTRUCT supplies this message-scoped paint HDC.
        unsafe {
            SetTextColor(draw.hDC, color);
        }
        // SAFETY: Changes only the live paint DC's text background mode.
        unsafe {
            SetBkMode(draw.hDC, TRANSPARENT.cast_signed());
        }
        // SAFETY: Live child HWND accepts a pointer-free font query.
        let font = unsafe { SendMessageW(draw.hwndItem, WM_GETFONT, 0, 0) };
        // SAFETY: The queried font remains retained by this live control throughout painting.
        let old_font =
            unsafe { SelectObject(draw.hDC, without_provenance_mut(font.cast_unsigned())) };
        let mut text = [0u16; 96];
        // SAFETY: This writable buffer has the supplied bound of 96 UTF-16 units.
        let length = unsafe { GetWindowTextW(draw.hwndItem, text.as_mut_ptr(), 96) };
        let mut rect = draw.rcItem;
        // SAFETY: Text buffer and writable rectangle live throughout the synchronous paint call.
        unsafe {
            DrawTextW(
                draw.hDC,
                text.as_ptr(),
                length.max(0),
                &raw mut rect,
                DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX,
            );
        }
        // SAFETY: Restore the previous borrowed font before leaving the paint scope.
        unsafe {
            SelectObject(draw.hDC, old_font);
        }
        if draw.itemState & ODS_FOCUS != 0 {
            rect.left += 4;
            rect.top += 4;
            rect.right -= 4;
            rect.bottom -= 4;
            // SAFETY: This focus rectangle stays inside the message-scoped button DC.
            unsafe {
                DrawFocusRect(draw.hDC, &raw const rect);
            }
        }
    });
}
fn make_font(pixels: i32, weight: i32, dpi: i32) -> HFONT {
    // SAFETY: Scalar font settings and static face name create an owned GDI font.
    unsafe {
        CreateFontW(
            -pixels.saturating_mul(dpi) / 96,
            0,
            0,
            0,
            weight,
            0,
            0,
            0,
            u32::from(DEFAULT_CHARSET),
            u32::from(OUT_DEFAULT_PRECIS),
            u32::from(CLIP_DEFAULT_PRECIS),
            u32::from(CLEARTYPE_QUALITY),
            u32::from(DEFAULT_PITCH),
            w!("Segoe UI"),
        )
    }
}
fn apply_font(control: HWND, font: HFONT) {
    // SAFETY: Live control retains this live font until replacement or destruction.
    unsafe {
        SendMessageW(control, WM_SETFONT, font.addr(), 1);
    }
}
fn delete_fonts(fonts: [HFONT; 3]) {
    for font in fonts.into_iter().filter(|font| !font.is_null()) {
        // SAFETY: These distinct owned fonts are no longer retained by live controls.
        unsafe {
            DeleteObject(font);
        }
    }
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

fn set_text(control: HWND, text: &str) {
    let text = wide(text);
    // SAFETY: All callers supply live controls on their creating UI thread;
    // the NUL-terminated UTF-16 allocation remains alive for the synchronous call.
    unsafe {
        SetWindowTextW(control, text.as_ptr());
    }
}

fn append(control: HWND, text: &str) {
    transcript::append(control, text, TEXT, false);
}

fn control(parent: HWND, class: *const u16, text: &str, style: u32, id: usize) -> io::Result<HWND> {
    let text = wide(text);
    // SAFETY: Parent belongs to this thread; class is a static system class,
    // text lives through the synchronous call, and child IDs are nonpointer IDs.
    let handle = unsafe {
        CreateWindowExW(
            0,
            class,
            text.as_ptr(),
            WS_CHILD | WS_VISIBLE | style,
            0,
            0,
            1,
            1,
            parent,
            without_provenance_mut(id),
            null_mut(),
            null(),
        )
    };
    if handle.is_null() {
        Err(io::Error::last_os_error())
    } else {
        Ok(handle)
    }
}

fn edit(parent: HWND, readonly: bool, id: usize) -> io::Result<HWND> {
    let style = WS_TABSTOP
        | WS_VSCROLL
        | u32::try_from(ES_MULTILINE | ES_AUTOVSCROLL).unwrap_or_default()
        | if readonly {
            u32::try_from(ES_READONLY).unwrap_or_default()
        } else {
            u32::try_from(ES_WANTRETURN).unwrap_or_default()
        };
    // SAFETY: Parent is live on this UI thread, EDIT is a system class, all
    // pointers are static or null, and no application-owned pointer is retained.
    let handle = unsafe {
        CreateWindowExW(
            0,
            w!("EDIT"),
            w!(""),
            WS_CHILD | WS_VISIBLE | style,
            0,
            0,
            1,
            1,
            parent,
            without_provenance_mut(id),
            null_mut(),
            null(),
        )
    };
    if handle.is_null() {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: The live EDIT control receives a bounded integer text limit.
    unsafe {
        SendMessageW(
            handle,
            EM_SETLIMITTEXT,
            if readonly { 4 * 1024 * 1024 } else { 32768 },
            0,
        );
    }
    // SAFETY: Static theme name is consumed synchronously by this live native control.
    unsafe {
        SetWindowTheme(handle, w!("DarkMode_Explorer"), null());
    }
    Ok(handle)
}

fn state_with<T>(action: impl FnOnce(&mut State) -> T) -> Option<T> {
    STATE.with(|state| state.try_borrow_mut().ok()?.as_mut().map(action))
}

impl State {
    fn create(
        window: HWND,
        commands: Sender<Command>,
        events: Receiver<Event>,
    ) -> io::Result<Self> {
        Ok(Self {
            window,
            title: control(window, w!("STATIC"), "Swift Bonsai 2", 0, 301)?,
            subtitle: control(
                window,
                w!("STATIC"),
                "Your private, local writing companion",
                0,
                0,
            )?,
            transcript: transcript::create(window, 201)?,
            reasoning: transcript::create(window, 202)?,
            input: edit(window, false, 203)?,
            send: control(
                window,
                w!("BUTTON"),
                "Send  ↑",
                WS_TABSTOP | u32::try_from(BS_OWNERDRAW).unwrap_or_default(),
                SEND,
            )?,
            stop: control(
                window,
                w!("BUTTON"),
                "Stop",
                WS_TABSTOP | u32::try_from(BS_OWNERDRAW).unwrap_or_default(),
                STOP,
            )?,
            new_chat: control(
                window,
                w!("BUTTON"),
                "＋  New conversation",
                WS_TABSTOP | u32::try_from(BS_OWNERDRAW).unwrap_or_default(),
                NEW,
            )?,
            thinking: control(
                window,
                w!("BUTTON"),
                "Show thinking",
                WS_TABSTOP | u32::try_from(BS_OWNERDRAW).unwrap_or_default(),
                THINKING,
            )?,
            retry: control(
                window,
                w!("BUTTON"),
                "Retry model",
                WS_TABSTOP | u32::try_from(BS_OWNERDRAW).unwrap_or_default(),
                RETRY,
            )?,
            status: control(
                window,
                w!("STATIC"),
                "Verifying model and starting Swift. This can take a moment...",
                0,
                0,
            )?,
            font: null_mut(),
            title_font: null_mut(),
            small_font: null_mut(),
            commands,
            events,
            ready: false,
            busy: false,
            retryable: false,
            show_thinking: false,
            chat_started: false,
        })
    }

    fn update_buttons(&self) {
        // SAFETY: The handles are controls owned by this State on their UI thread;
        // EnableWindow consumes only integer flags and retains no Rust references.
        unsafe {
            EnableWindow(self.send, i32::from(self.ready && !self.busy));
        }
        // SAFETY: Same control lifetime/thread contract as above.
        unsafe {
            EnableWindow(self.new_chat, i32::from(self.ready && !self.busy));
        }
        // SAFETY: Same control lifetime/thread contract as above.
        unsafe {
            EnableWindow(self.input, i32::from(self.ready && !self.busy));
        }
        // SAFETY: Same control lifetime/thread contract as above.
        unsafe {
            EnableWindow(self.stop, i32::from(self.busy));
        }
        // SAFETY: Retry is a live UI-thread child and only scalar flags are passed.
        unsafe {
            EnableWindow(self.retry, i32::from(self.retryable));
        }
        // SAFETY: Toggle the live retry button without retaining references.
        unsafe {
            ShowWindow(self.retry, if self.retryable { SW_SHOW } else { SW_HIDE });
        }
    }

    fn layout(&self) {
        let mut bounds = RECT::default();
        // SAFETY: Live UI-thread window and writable rectangle storage.
        unsafe {
            GetClientRect(self.window, &raw mut bounds);
        }
        // SAFETY: Live window queried only for scalar DPI.
        let dpi = i32::try_from(unsafe { GetDpiForWindow(self.window) }.max(96)).unwrap_or(96);
        let p = |v: i32| v.saturating_mul(dpi) / 96;
        let width = bounds.right.max(p(480));
        let height = bounds.bottom.max(p(520));
        let pad = p(32);
        let content = width - 2 * pad;
        let top = p(148);
        let bottom = height - p(184);
        let split = if self.show_thinking {
            (bottom - top) / 3
        } else {
            0
        };
        let body_height = bottom - top - split;
        let place = |handle, x, y, w: i32, h: i32| {
            // SAFETY: All handles are live UI-thread children, and layout coordinates are scalar.
            unsafe {
                MoveWindow(handle, x, y, w.max(1), h.max(1), 1);
            }
        };
        place(self.title, pad, p(25), content, p(46));
        place(self.subtitle, pad + p(1), p(79), content, p(20));
        place(self.new_chat, pad, p(106), p(190), p(30));
        place(self.thinking, pad + p(202), p(106), p(145), p(30));
        place(self.retry, width - pad - p(145), p(106), p(145), p(30));
        place(
            self.transcript,
            pad + p(20),
            top + p(18),
            content - p(40),
            body_height - p(36),
        );
        place(
            self.reasoning,
            pad + p(20),
            top + body_height + p(28),
            content - p(40),
            split - p(46),
        );
        // SAFETY: Live child HWND, scalar visibility flag.
        unsafe {
            ShowWindow(
                self.reasoning,
                if self.show_thinking { SW_SHOW } else { SW_HIDE },
            );
        }
        place(
            self.input,
            pad + p(20),
            bottom + p(34),
            content - p(160),
            p(82),
        );
        place(
            self.send,
            width - pad - p(121),
            bottom + p(31),
            p(101),
            p(42),
        );
        place(
            self.stop,
            width - pad - p(121),
            bottom + p(82),
            p(101),
            p(28),
        );
        place(
            self.status,
            pad + p(2),
            height - p(38),
            content - p(4),
            p(32),
        );
        // SAFETY: Invalidate the owned window; null means the complete client area.
        unsafe {
            InvalidateRect(self.window, null(), 1);
        }
    }

    fn set_font(&mut self) {
        // SAFETY: Live HWND on this UI thread.
        let dpi = i32::try_from(unsafe { GetDpiForWindow(self.window) }.max(96)).unwrap_or(96);
        let font = make_font(17, FW_NORMAL.cast_signed(), dpi);
        let title = make_font(34, 600, dpi);
        let small = make_font(13, FW_NORMAL.cast_signed(), dpi);
        if font.is_null() || title.is_null() || small.is_null() {
            delete_fonts([font, title, small]);
            return;
        }
        for control in [
            self.input,
            self.send,
            self.stop,
            self.new_chat,
            self.thinking,
            self.retry,
        ] {
            apply_font(control, font);
        }
        transcript::set_font(self.transcript);
        transcript::set_font(self.reasoning);
        apply_font(self.title, title);
        apply_font(self.subtitle, small);
        apply_font(self.status, small);
        delete_fonts([self.font, self.title_font, self.small_font]);
        self.font = font;
        self.title_font = title;
        self.small_font = small;
    }

    fn submit(&mut self) {
        if !self.ready || self.busy {
            return;
        }
        // SAFETY: input is a live EDIT on this thread; no pointer is passed.
        let count = unsafe { GetWindowTextLengthW(self.input) };
        let mut buffer = vec![0u16; usize::try_from(count.max(0)).unwrap_or_default() + 1];
        // SAFETY: buffer is writable for count+1 UTF-16 units, count is queried
        // from the same UI-thread control, and the call writes at most that bound.
        let copied =
            unsafe { GetWindowTextW(self.input, buffer.as_mut_ptr(), count.saturating_add(1)) };
        let text =
            String::from_utf16_lossy(&buffer[..usize::try_from(copied.max(0)).unwrap_or_default()]);
        let text = text.trim();
        if text.is_empty() {
            return;
        }
        if self.commands.send(Command::Send(text.to_owned())).is_err() {
            self.fail("The model worker stopped. Close and restart the demo.");
            return;
        }
        if !self.chat_started {
            set_text(self.transcript, "");
            self.chat_started = true;
        }
        transcript::turn(self.transcript, "You", 0x00e8_c799);
        append(self.transcript, text);
        transcript::turn(self.transcript, "Swift Bonsai 2", ACCENT);
        transcript::scroll_to_end(self.transcript);
        set_text(self.input, "");
        set_text(self.reasoning, "");
        transcript::turn(self.reasoning, "Thinking", MUTED);
        self.busy = true;
        set_text(
            self.status,
            "Thinking...  Enter sends; Shift+Enter adds a line. Stop requires restarting the demo.",
        );
        self.update_buttons();
    }

    fn fail(&mut self, message: &str) {
        self.ready = false;
        self.busy = false;
        self.retryable = false;
        set_text(self.status, message);
        transcript::turn(self.transcript, "Demo status", MUTED);
        transcript::append(self.transcript, message, MUTED, false);
        self.update_buttons();
    }

    fn drain_events(&mut self) {
        for _ in 0..256 {
            let Ok(event) = self.events.try_recv() else {
                break;
            };
            match event {
                Event::Starting => {
                    self.ready = false;
                    self.busy = false;
                    self.retryable = false;
                    set_text(
                        self.status,
                        "Starting Swift. Checking memory and verifying the local model...",
                    );
                    self.update_buttons();
                }
                Event::Retryable(message) => {
                    self.fail(&message);
                    self.retryable = true;
                    set_text(
                        self.status,
                        "More free memory is needed. Close a few applications, then choose Retry model.",
                    );
                    self.update_buttons();
                }
                Event::Ready => {
                    self.ready = true;
                    self.retryable = false;
                    set_text(
                        self.status,
                        "Ready. Enter to send; Shift+Enter for a new line.",
                    );
                    self.update_buttons();
                    // SAFETY: Query only this UI thread's live focus handle.
                    let focus = unsafe { GetFocus() };
                    if focus != self.transcript && focus != self.reasoning {
                        // SAFETY: The composer is enabled; readers keep keyboard
                        // focus in the transcript so copying still works.
                        unsafe { SetFocus(self.input) };
                    }
                }
                Event::Text(text) => {
                    append(self.transcript, &text);
                    set_text(self.status, "Writing...");
                }
                Event::Reasoning(text) => {
                    transcript::append(self.reasoning, &text, MUTED, false);
                }
                Event::Complete => {
                    self.busy = false;
                    set_text(
                        self.status,
                        "Ready. Enter to send; Shift+Enter for a new line.",
                    );
                    self.update_buttons();
                    // SAFETY: Query only this UI thread's live focus handle.
                    let focus = unsafe { GetFocus() };
                    if focus != self.transcript && focus != self.reasoning {
                        // SAFETY: The composer is enabled; readers keep keyboard
                        // focus in the transcript so copying still works.
                        unsafe { SetFocus(self.input) };
                    }
                }
                Event::Failed(message) => self.fail(&message),
            }
        }
    }
}

#[allow(
    clippy::too_many_lines,
    reason = "Keep audited native message lifetimes and dispatch branches together"
)]
unsafe extern "system" fn procedure(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_CTLCOLOREDIT | WM_CTLCOLORSTATIC | WM_CTLCOLORBTN => {
            return color_control(wparam, lparam);
        }
        WM_PAINT => {
            paint(window);
            return 0;
        }
        WM_ERASEBKGND => return 1,
        WM_GETMINMAXINFO if lparam != 0 => {
            // SAFETY: The system supplies writable MINMAXINFO for this synchronous callback.
            let bounds =
                unsafe { &mut *without_provenance_mut::<MINMAXINFO>(lparam.cast_unsigned()) };
            // SAFETY: This live HWND is queried only for scalar DPI.
            let dpi = i32::try_from(unsafe { GetDpiForWindow(window) }.max(96)).unwrap_or(96);
            bounds.ptMinTrackSize.x = 680_i32.saturating_mul(dpi) / 96;
            bounds.ptMinTrackSize.y = 650_i32.saturating_mul(dpi) / 96;
            return 0;
        }
        WM_DRAWITEM if lparam != 0 => {
            // SAFETY: The system supplies aligned DRAWITEMSTRUCT storage valid for this synchronous callback.
            let draw =
                unsafe { &*without_provenance_mut::<DRAWITEMSTRUCT>(lparam.cast_unsigned()) };
            draw_button(draw);
            return 1;
        }
        _ => {}
    }
    if message == WM_DESTROY {
        // SAFETY: Called on the creating thread; posts a scalar quit code to its loop.
        unsafe {
            PostQuitMessage(0);
        }
        return 0;
    }
    let handled = state_with(|state| match message {
        WM_SIZE => {
            state.layout();
            true
        }
        WM_DPICHANGED => {
            state.set_font();
            state.layout();
            true
        }
        WM_TIMER if wparam == TIMER => {
            state.drain_events();
            true
        }
        WM_COMMAND => {
            match wparam & 0xffff {
                SEND => state.submit(),
                RETRY if state.retryable => {
                    if state.commands.send(Command::Retry).is_ok() {
                        state.retryable = false;
                        state.ready = false;
                        state.busy = false;
                        set_text(
                            state.status,
                            "Starting Swift. Rechecking available memory...",
                        );
                        state.update_buttons();
                    } else {
                        state.fail("The model worker stopped. Close and restart the demo.");
                    }
                }
                STOP if state.busy => {
                    let _result = state.commands.send(Command::Stop);
                    state.fail(
                        "Stopped. Close and restart the demo to start a fresh model session.",
                    );
                }
                NEW if state.ready && !state.busy => {
                    let _result = state.commands.send(Command::NewChat);
                    transcript::welcome(state.transcript);
                    state.chat_started = false;
                    set_text(state.reasoning, "");
                    // SAFETY: The composer is an enabled live control on this thread.
                    unsafe { SetFocus(state.input) };
                }
                THINKING => {
                    state.show_thinking = !state.show_thinking;
                    set_text(
                        state.thinking,
                        if state.show_thinking {
                            "Hide thinking"
                        } else {
                            "Show thinking"
                        },
                    );
                    state.layout();
                }
                _ => {}
            }
            true
        }
        WM_CLOSE => {
            let _result = state.commands.send(Command::Shutdown);
            // SAFETY: These are this UI thread's timer and top-level window;
            // WM_DESTROY is handled without borrowing State during reentrancy.
            unsafe {
                KillTimer(window, TIMER);
            }
            // SAFETY: The top-level HWND is live and owned by this thread.
            unsafe {
                DestroyWindow(window);
            }
            true
        }
        _ => false,
    })
    .unwrap_or(false);
    if handled {
        return 0;
    }
    // SAFETY: Forward the unmodified system message to the standard window proc.
    unsafe { DefWindowProcW(window, message, wparam, lparam) }
}

fn create_window() -> io::Result<HWND> {
    // SAFETY: Called before this application creates any window; failure means
    // the process inherited an existing awareness context and can continue.
    unsafe {
        SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }
    // SAFETY: Null requests the current process module, a borrowed process-lifetime handle.
    let instance = unsafe { GetModuleHandleW(null()) };
    // SAFETY: System icon/cursor IDs and null instance select shared system resources.
    let icon = unsafe { LoadIconW(null_mut(), IDI_APPLICATION) };
    // SAFETY: Same shared-resource lifetime contract as above.
    let cursor = unsafe { LoadCursorW(null_mut(), IDC_ARROW) };

    let class = WNDCLASSW {
        lpfnWndProc: Some(procedure),
        hInstance: instance,
        hIcon: icon,
        hCursor: cursor,
        hbrBackground: null_mut(),
        lpszClassName: w!("SwiftBonsaiRustChat"),
        ..WNDCLASSW::default()
    };
    // SAFETY: Class contains only process-lifetime resources and a static callback.
    if unsafe { RegisterClassW(&raw const class) } == 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: Registered class/callback and title are static, the module handle
    // is borrowed, and this is an owned top-level window with no retained Rust pointer.
    let window = unsafe {
        CreateWindowExW(
            WS_EX_CONTROLPARENT,
            w!("SwiftBonsaiRustChat"),
            w!("Swift Bonsai 2 — Native Rust Chat"),
            WS_OVERLAPPEDWINDOW | WS_CLIPCHILDREN,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            1100,
            870,
            null_mut(),
            null_mut(),
            instance,
            null(),
        )
    };
    if window.is_null() {
        return Err(io::Error::last_os_error());
    }
    let dark: i32 = 1;
    // SAFETY: Attribute 20 takes a four-byte BOOL, alive through this call; unsupported Windows versions may reject it harmlessly.
    unsafe {
        DwmSetWindowAttribute(window, 20, (&raw const dark).cast(), 4);
    }
    let corners: u32 = 2;
    // SAFETY: Attribute 33 takes a four-byte corner preference, alive through the synchronous call.
    unsafe {
        DwmSetWindowAttribute(window, 33, (&raw const corners).cast(), 4);
    }
    Ok(window)
}

struct WindowLifetime(HWND);

impl Drop for WindowLifetime {
    fn drop(&mut self) {
        // SAFETY: This handle belongs to the current UI thread. On early startup
        // errors, destroy all children before unloading the Rich Edit module.
        if unsafe { IsWindow(self.0) } != 0 {
            // SAFETY: The guard owns this live window on its creating thread.
            unsafe { DestroyWindow(self.0) };
        }
    }
}

pub(super) fn run(config: Config) -> io::Result<()> {
    let _rich_edit = transcript::Library::load()?;
    PALETTE.with(|palette| {
        *palette.borrow_mut() = Some(Palette::new()?);
        Ok::<(), io::Error>(())
    })?;
    let window = create_window()?;
    let _window_lifetime = WindowLifetime(window);
    let (commands, incoming_commands) = mpsc::channel();
    let (events, incoming_events) = mpsc::channel();
    let mut state = State::create(window, commands.clone(), incoming_events)?;
    state.set_font();
    state.layout();
    state.update_buttons();
    transcript::welcome(state.transcript);
    STATE.with(|slot| *slot.borrow_mut() = Some(state));
    let worker = std::thread::Builder::new()
        .name("swift-chat-client".to_owned())
        .spawn(move || client::run(config, incoming_commands, events))?;
    // SAFETY: The window is live; the system posts WM_TIMER to this thread,
    // using no callback or Rust pointer; the timer is killed before destruction.
    unsafe {
        SetTimer(window, TIMER, 50, None);
    }
    // SAFETY: Show this application's live top-level window on its owning thread.
    unsafe {
        ShowWindow(window, SW_SHOW);
    }
    let mut message = MSG::default();
    loop {
        // SAFETY: message is writable storage on this thread. Null HWND reads
        // its message queue, and no input pointers outlive this synchronous call.
        let result = unsafe { GetMessageW(&raw mut message, null_mut(), 0, 0) };
        if result <= 0 {
            break;
        }
        if message.message == WM_KEYDOWN && message.wParam == usize::from(VK_RETURN) {
            // SAFETY: Reads this thread's focus and scalar keyboard state.
            let focus = unsafe { GetFocus() };
            // SAFETY: VK_SHIFT is a valid virtual-key identifier.
            let shift = unsafe { GetKeyState(i32::from(VK_SHIFT)) };
            if shift >= 0
                && state_with(|state| {
                    if focus == state.input {
                        state.submit();
                        true
                    } else {
                        false
                    }
                })
                .unwrap_or(false)
            {
                continue;
            }
        }
        // SAFETY: Both HWND and MSG are live on this thread; navigation is
        // confined to this top-level window and its system child controls.
        if unsafe { IsDialogMessageW(window, &raw const message) } == 0 {
            // SAFETY: Forward a valid message obtained from GetMessageW.
            unsafe {
                TranslateMessage(&raw const message);
            }
            // SAFETY: Dispatch the same valid message to its registered procedure.
            unsafe {
                DispatchMessageW(&raw const message);
            }
        }
    }
    let _result = commands.send(Command::Shutdown);
    let _result = worker.join();
    STATE.with(|slot| {
        if let Some(state) = slot.borrow_mut().take() {
            delete_fonts([state.font, state.title_font, state.small_font]);
        }
    });
    PALETTE.with(|palette| {
        palette.borrow_mut().take();
    });
    Ok(())
}

pub(super) fn show_error(message: &str) {
    let message = wide(message);
    // SAFETY: No parent is supplied; message and title buffers remain valid for
    // the synchronous modal call; the system owns all dialog internals.
    unsafe {
        MessageBoxW(
            null_mut(),
            message.as_ptr(),
            w!("Swift Bonsai 2"),
            MB_OK | MB_ICONERROR,
        );
    }
}
