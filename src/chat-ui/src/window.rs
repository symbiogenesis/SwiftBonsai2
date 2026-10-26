//! Win32 controls and message dispatch, confined to the main UI thread.
//!
//! Control handles never cross threads. Reentrant messages use `try_borrow_mut`
//! rather than producing overlapping references to the window state.
#![allow(
    clippy::redundant_pub_crate,
    reason = "Keep the audited native boundary explicitly parent-only."
)]

use crate::{
    client::{self, Command, Completion, Config, Event},
    speech,
};
mod microphone;
#[cfg(test)]
mod tests;
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
            BeginPaint, CLEARTYPE_QUALITY, CLIP_DEFAULT_PRECIS, COLOR_BTNFACE, COLOR_BTNTEXT,
            COLOR_GRAYTEXT, COLOR_HIGHLIGHT, COLOR_HIGHLIGHTTEXT, COLOR_WINDOW, COLOR_WINDOWTEXT,
            CreateFontW, CreateSolidBrush, DC_PEN, DEFAULT_CHARSET, DEFAULT_PITCH, DT_CENTER,
            DT_SINGLELINE, DT_VCENTER, DeleteObject, DrawFocusRect, DrawTextW, EndPaint, FW_NORMAL,
            FillRect, GetStockObject, GetSysColor, HBRUSH, HDC, HFONT, InvalidateRect,
            OUT_DEFAULT_PRECIS, PAINTSTRUCT, RoundRect, SelectObject, SetBkColor, SetBkMode,
            SetDCPenColor, SetTextColor, TRANSPARENT,
        },
        System::LibraryLoader::GetModuleHandleW,
        UI::{
            Accessibility::{HCF_HIGHCONTRASTON, HIGHCONTRASTW},
            Controls::{
                DRAWITEMSTRUCT, EM_REPLACESEL, EM_SETLIMITTEXT, ODS_DISABLED, ODS_FOCUS,
                ODS_SELECTED, ODT_BUTTON, SetWindowTheme,
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
                LoadIconW, MB_ICONERROR, MB_ICONINFORMATION, MB_OK, MINMAXINFO, MSG, MessageBoxW,
                MoveWindow, PostQuitMessage, RegisterClassW, SPI_GETHIGHCONTRAST, SW_HIDE, SW_SHOW,
                SWP_NOACTIVATE, SWP_NOZORDER, SendMessageW, SetTimer, SetWindowPos, SetWindowTextW,
                ShowWindow, SystemParametersInfoW, TranslateMessage, WM_CLOSE, WM_COMMAND,
                WM_CTLCOLORBTN, WM_CTLCOLOREDIT, WM_CTLCOLORSTATIC, WM_DESTROY, WM_DPICHANGED,
                WM_DRAWITEM, WM_ERASEBKGND, WM_GETFONT, WM_KEYDOWN, WM_PAINT, WM_SETFONT,
                WM_SETTINGCHANGE, WM_SIZE, WM_SYSCOLORCHANGE, WM_THEMECHANGED, WM_TIMER, WNDCLASSW,
                WS_CHILD, WS_CLIPCHILDREN, WS_EX_CONTROLPARENT, WS_OVERLAPPEDWINDOW, WS_TABSTOP,
                WS_VISIBLE, WS_VSCROLL,
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
const DICTATE: usize = 106;
const READ_ALOUD: usize = 107;
const DEFAULT_VOICE: &str = "A warm, clear, natural adult voice with a calm conversational pace.";
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
    reason = "Readiness, generation, retry, speech capabilities and active speech operations are independent UI state flags"
)]
struct State {
    window: HWND,
    title: HWND,
    subtitle: HWND,
    transcript_label: HWND,
    transcript: HWND,
    reasoning_label: HWND,
    reasoning: HWND,
    input_label: HWND,
    input: HWND,
    send: HWND,
    stop: HWND,
    new_chat: HWND,
    thinking: HWND,
    retry: HWND,
    dictate: HWND,
    read_aloud: HWND,
    speech_status: HWND,
    voice_label: HWND,
    voice_profile: HWND,
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
    speech: Option<speech::Session>,
    speech_input: bool,
    speech_output: bool,
    canary_configured: bool,
    tts_configured: bool,
    listening: bool,
    dictation_requested: bool,
    microphone: Option<microphone::Microphone>,
    microphone_generation: Option<u64>,
    speaking: bool,
    answer_complete: bool,
    last_answer: String,
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
#[derive(Clone, Copy, PartialEq, Eq)]
struct ColorScheme {
    background: u32,
    panel: u32,
    border: u32,
    text: u32,
    muted: u32,
    accent: u32,
    accent_text: u32,
    button: u32,
    button_text: u32,
    disabled: u32,
    high_contrast: bool,
}
impl ColorScheme {
    const SAGE: Self = Self {
        background: BACKGROUND,
        panel: PANEL,
        border: BORDER,
        text: TEXT,
        muted: MUTED,
        accent: ACCENT,
        accent_text: BACKGROUND,
        button: BUTTON,
        button_text: TEXT,
        disabled: 0x0060_6b58,
        high_contrast: false,
    };

    fn current() -> Self {
        let mut contrast = HIGHCONTRASTW {
            cbSize: u32::try_from(size_of::<HIGHCONTRASTW>()).unwrap_or_default(),
            ..HIGHCONTRASTW::default()
        };
        // SAFETY: The OS writes only this correctly sized SDK structure during the call.
        let available = unsafe {
            SystemParametersInfoW(
                SPI_GETHIGHCONTRAST,
                contrast.cbSize,
                (&raw mut contrast).cast(),
                0,
            )
        };
        if available == 0 || contrast.dwFlags & HCF_HIGHCONTRASTON == 0 {
            return Self::SAGE;
        }
        Self::system_contrast()
    }

    fn system_contrast() -> Self {
        // SAFETY: Each index is a documented system color; no pointer or ownership crosses the call.
        let background = unsafe { GetSysColor(COLOR_WINDOW) };
        // SAFETY: COLOR_WINDOWTEXT is a documented system color index.
        let text = unsafe { GetSysColor(COLOR_WINDOWTEXT) };
        // SAFETY: COLOR_BTNFACE is a documented system color index.
        let button = unsafe { GetSysColor(COLOR_BTNFACE) };
        // SAFETY: COLOR_BTNTEXT is a documented system color index.
        let button_text = unsafe { GetSysColor(COLOR_BTNTEXT) };
        // SAFETY: COLOR_HIGHLIGHT is a documented system color index.
        let accent = unsafe { GetSysColor(COLOR_HIGHLIGHT) };
        // SAFETY: COLOR_HIGHLIGHTTEXT is a documented system color index.
        let accent_text = unsafe { GetSysColor(COLOR_HIGHLIGHTTEXT) };
        // SAFETY: COLOR_GRAYTEXT is a documented system color index.
        let disabled = unsafe { GetSysColor(COLOR_GRAYTEXT) };
        Self {
            background,
            panel: background,
            border: text,
            text,
            muted: text,
            accent,
            accent_text,
            button,
            button_text,
            disabled,
            high_contrast: true,
        }
    }
}

struct Palette {
    colors: ColorScheme,
    background: Brush,
    panel: Brush,
    button: Brush,
    accent: Brush,
}
impl Palette {
    fn new() -> io::Result<Self> {
        Self::from_colors(ColorScheme::current())
    }

    fn from_colors(colors: ColorScheme) -> io::Result<Self> {
        Ok(Self {
            colors,
            background: Brush::new(colors.background)?,
            panel: Brush::new(colors.panel)?,
            button: Brush::new(colors.button)?,
            accent: Brush::new(colors.accent)?,
        })
    }
}
fn palette_with<T>(action: impl FnOnce(&Palette) -> T) -> Option<T> {
    PALETTE.with(|palette| palette.try_borrow().ok()?.as_ref().map(action))
}
fn colors() -> ColorScheme {
    palette_with(|palette| palette.colors).unwrap_or_else(ColorScheme::current)
}
fn display_color(color: u32) -> u32 {
    let colors = colors();
    if colors.high_contrast {
        colors.text
    } else {
        color
    }
}
fn color_control(wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let dc: HDC = without_provenance_mut(wparam);
    let control: HWND = without_provenance_mut(lparam.cast_unsigned());
    // SAFETY: WM_CTLCOLOR supplies a live control and message-scoped paint HDC.
    let id = unsafe { GetDlgCtrlID(control) };
    let editor = matches!(id, 201..=204);
    let colors = colors();
    let foreground = if id == 202 || (!editor && id != 301) {
        colors.muted
    } else {
        colors.text
    };
    // SAFETY: dc is the live paint context supplied by this synchronous message.
    unsafe {
        SetTextColor(dc, foreground);
    }
    // SAFETY: Same message-scoped HDC; only scalar colors are passed.
    unsafe {
        SetBkColor(
            dc,
            if editor {
                colors.panel
            } else {
                colors.background
            },
        );
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
    let top = px(280);
    let bottom = bounds.bottom - px(184);
    let split = if thinking { (bottom - top) / 3 } else { 0 };
    palette_with(|p| {
        // SAFETY: Brush and paint DC are live during this synchronous paint scope.
        unsafe {
            FillRect(dc, &raw const bounds, p.background.0);
        }
        let voice = RECT {
            left: pad,
            top: px(208),
            right: bounds.right - pad,
            bottom: px(266),
        };
        round_box(dc, voice, p.panel.0, p.colors.border, px(12));
        let body = RECT {
            left: pad,
            top,
            right: bounds.right - pad,
            bottom: bottom - split,
        };
        round_box(dc, body, p.panel.0, p.colors.border, px(16));
        if thinking {
            let reasoning = RECT {
                left: pad,
                top: bottom - split + px(10),
                right: bounds.right - pad,
                bottom,
            };
            round_box(dc, reasoning, p.panel.0, p.colors.border, px(16));
        }
        let composer = RECT {
            left: pad,
            top: bottom + px(16),
            right: bounds.right - pad,
            bottom: bounds.bottom - px(49),
        };
        round_box(dc, composer, p.panel.0, p.colors.border, px(16));
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
            if primary && !disabled {
                p.colors.accent
            } else {
                p.colors.border
            },
            12,
        );
        let color = if disabled {
            p.colors.disabled
        } else if primary && !selected {
            p.colors.accent_text
        } else {
            p.colors.button_text
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
                DT_CENTER | DT_VCENTER | DT_SINGLELINE,
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

fn control_text(control: HWND) -> String {
    // SAFETY: Callers supply live native edits owned by this UI thread.
    let count = unsafe { GetWindowTextLengthW(control) };
    let mut buffer = vec![0u16; usize::try_from(count.max(0)).unwrap_or_default() + 1];
    // SAFETY: The allocation covers the queried text length plus terminator;
    // no intervening UI dispatch can mutate this same-thread edit.
    let copied = unsafe { GetWindowTextW(control, buffer.as_mut_ptr(), count.saturating_add(1)) };
    String::from_utf16_lossy(&buffer[..usize::try_from(copied.max(0)).unwrap_or_default()])
}

fn append(control: HWND, text: &str) {
    transcript::append(control, text, TEXT, false);
}

fn place(handle: HWND, x: i32, y: i32, width: i32, height: i32) {
    // SAFETY: Callers supply live UI-thread controls and bounded scalar coordinates.
    unsafe {
        MoveWindow(handle, x, y, width.max(1), height.max(1), 1);
    }
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

fn button(parent: HWND, text: &str, id: usize) -> io::Result<HWND> {
    control(
        parent,
        w!("BUTTON"),
        text,
        WS_TABSTOP | u32::try_from(BS_OWNERDRAW).unwrap_or_default(),
        id,
    )
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
    // SAFETY: Theme pointers are static or null and consumed synchronously.
    unsafe {
        SetWindowTheme(
            handle,
            if colors().high_contrast {
                null()
            } else {
                w!("DarkMode_Explorer")
            },
            null(),
        );
    }
    Ok(handle)
}

fn voice_profile(parent: HWND) -> io::Result<HWND> {
    let control = edit(parent, false, 204)?;
    // SAFETY: This live native edit accepts a bounded scalar text limit.
    unsafe { SendMessageW(control, EM_SETLIMITTEXT, 1024, 0) };
    set_text(control, DEFAULT_VOICE);
    Ok(control)
}

fn state_with<T>(action: impl FnOnce(&mut State) -> T) -> Option<T> {
    STATE.with(|state| state.try_borrow_mut().ok()?.as_mut().map(action))
}

impl State {
    fn create(
        window: HWND,
        commands: Sender<Command>,
        events: Receiver<Event>,
        config: &Config,
        microphone: Option<microphone::Microphone>,
    ) -> io::Result<Self> {
        let speech = speech::start(config.canary.clone(), config.qwen_tts.clone()).ok();
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
            // Native accessibility proxies associate each edit with its preceding
            // static label. Keep these label/control pairs adjacent in child order.
            transcript_label: control(window, w!("STATIC"), "&Conversation", 0, 401)?,
            transcript: transcript::create(window, 201)?,
            reasoning_label: control(window, w!("STATIC"), "Reasoning details", 0, 402)?,
            reasoning: transcript::create(window, 202)?,
            input_label: control(window, w!("STATIC"), "M&essage", 0, 403)?,
            input: edit(window, false, 203)?,
            send: button(window, "&Send  ↑", SEND)?,
            stop: button(window, "S&top", STOP)?,
            new_chat: button(window, "＋  &New conversation", NEW)?,
            thinking: button(window, "Show t&hinking", THINKING)?,
            retry: button(window, "&Retry model", RETRY)?,
            dictate: button(window, "&Microphone: off", DICTATE)?,
            read_aloud: button(window, "Read a&loud", READ_ALOUD)?,
            speech_status: control(
                window,
                w!("STATIC"),
                if speech.is_some() {
                    "Checking speech capabilities..."
                } else {
                    "Speech unavailable."
                },
                0,
                404,
            )?,
            voice_label: control(window, w!("STATIC"), "&Voice profile", 0, 405)?,
            voice_profile: voice_profile(window)?,
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
            speech,
            speech_input: false,
            speech_output: false,
            canary_configured: config.canary.is_some(),
            tts_configured: config.qwen_tts.is_some(),
            listening: false,
            dictation_requested: false,
            microphone,
            microphone_generation: None,
            speaking: false,
            answer_complete: false,
            last_answer: String::new(),
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
        // SAFETY: Speech buttons are live controls; capability and lifecycle flags are scalar.
        unsafe {
            EnableWindow(
                self.dictate,
                i32::from(self.dictation_requested || (!self.busy && !self.speaking)),
            );
        }
        // SAFETY: The native setting stays fixed while synthesis or playback is active.
        unsafe { EnableWindow(self.voice_profile, i32::from(!self.speaking)) };
        // SAFETY: Live speech control; readout is available only for retained visible answer text.
        unsafe {
            EnableWindow(self.read_aloud, i32::from(self.speaking || !self.busy));
        }
        set_text(
            self.dictate,
            if self.dictation_requested {
                "&Microphone: on"
            } else {
                "&Microphone: off"
            },
        );
        set_text(
            self.read_aloud,
            if self.speaking {
                "Stop s&peech"
            } else {
                "Read a&loud"
            },
        );
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
        let top = p(280);
        let bottom = height - p(184);
        let split = if self.show_thinking {
            (bottom - top) / 3
        } else {
            0
        };
        let body_height = bottom - top - split;
        place(self.title, pad, p(25), content, p(46));
        place(self.subtitle, pad + p(1), p(79), content, p(20));
        place(self.new_chat, pad, p(106), p(190), p(30));
        place(self.thinking, pad + p(202), p(106), p(145), p(30));
        place(self.retry, width - pad - p(145), p(106), p(145), p(30));
        place(self.dictate, pad, p(146), p(168), p(32));
        place(self.read_aloud, pad + p(180), p(146), p(145), p(32));
        place(
            self.speech_status,
            pad + p(337),
            p(146),
            content - p(337),
            p(42),
        );
        self.layout_voice_profile(width, dpi);
        place(
            self.transcript_label,
            pad + p(20),
            top + p(9),
            content - p(40),
            p(20),
        );
        place(
            self.transcript,
            pad + p(20),
            top + p(36),
            content - p(40),
            body_height - p(52),
        );
        place(
            self.reasoning_label,
            pad + p(20),
            top + body_height + p(19),
            content - p(40),
            p(20),
        );
        place(
            self.reasoning,
            pad + p(20),
            top + body_height + p(44),
            content - p(40),
            split - p(58),
        );
        // SAFETY: Live child label, scalar visibility flag mirrors its associated edit.
        unsafe {
            ShowWindow(
                self.reasoning_label,
                if self.show_thinking { SW_SHOW } else { SW_HIDE },
            );
        }
        // SAFETY: Live child HWND, scalar visibility flag.
        unsafe {
            ShowWindow(
                self.reasoning,
                if self.show_thinking { SW_SHOW } else { SW_HIDE },
            );
        }
        self.layout_composer(width, height, dpi);
        // SAFETY: Invalidate the owned window; null means the complete client area.
        unsafe {
            InvalidateRect(self.window, null(), 1);
        }
    }

    fn layout_composer(&self, width: i32, height: i32, dpi: i32) {
        let p = |value: i32| value.saturating_mul(dpi) / 96;
        let pad = p(32);
        let content = width - 2 * pad;
        let bottom = height - p(184);
        place(
            self.input_label,
            pad + p(20),
            bottom + p(24),
            content - p(160),
            p(20),
        );
        place(
            self.input,
            pad + p(20),
            bottom + p(48),
            content - p(160),
            p(62),
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
    }

    fn layout_voice_profile(&self, width: i32, dpi: i32) {
        let p = |value: i32| value.saturating_mul(dpi) / 96;
        let pad = p(32);
        let content = width - 2 * pad;
        place(self.voice_label, pad + p(2), p(184), content - p(4), p(20));
        place(
            self.voice_profile,
            pad + p(12),
            p(216),
            content - p(24),
            p(42),
        );
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
            self.dictate,
            self.read_aloud,
            self.voice_profile,
        ] {
            apply_font(control, font);
        }
        transcript::set_font(self.transcript);
        transcript::set_font(self.reasoning);
        apply_font(self.title, title);
        apply_font(self.subtitle, small);
        apply_font(self.status, small);
        apply_font(self.speech_status, small);
        apply_font(self.voice_label, small);
        apply_font(self.transcript_label, small);
        apply_font(self.reasoning_label, small);
        apply_font(self.input_label, small);
        delete_fonts([self.font, self.title_font, self.small_font]);
        self.font = font;
        self.title_font = title;
        self.small_font = small;
    }

    fn refresh_theme(&self) {
        let Ok(palette) = Palette::new() else {
            return;
        };
        if palette.colors == colors() {
            return;
        }
        PALETTE.with(|slot| *slot.borrow_mut() = Some(palette));
        let colors = colors();
        set_dark_frame(self.window, !colors.high_contrast);
        for control in [self.input, self.voice_profile] {
            // SAFETY: Live native edits and static/null theme pointers; calls are synchronous.
            unsafe {
                SetWindowTheme(
                    control,
                    if colors.high_contrast {
                        null()
                    } else {
                        w!("DarkMode_Explorer")
                    },
                    null(),
                );
            }
        }
        transcript::refresh_theme(self.transcript);
        transcript::refresh_theme(self.reasoning);
        // SAFETY: Invalidate this owned window and all visible controls through layout.
        unsafe { InvalidateRect(self.window, null(), 1) };
        self.layout();
    }

    fn submit(&mut self) {
        if !self.ready || self.busy {
            return;
        }
        let text = control_text(self.input);
        let text = text.trim();
        if text.is_empty() {
            return;
        }
        self.stop_speech();
        self.last_answer.clear();
        self.answer_complete = false;
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
        self.stop_speech();
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
                    self.last_answer.push_str(&text);
                    append(self.transcript, &text);
                    set_text(self.status, "Writing...");
                }
                Event::Reasoning(text) => {
                    transcript::append(self.reasoning, &text, MUTED, false);
                }
                Event::Complete(completion) => {
                    self.busy = false;
                    self.answer_complete = true;
                    set_text(
                        self.status,
                        match completion {
                            Completion::Finished => {
                                "Ready. Enter to send; Shift+Enter for a new line."
                            }
                            Completion::TokenLimit => {
                                "Response reached its token limit and may be incomplete. You can ask Swift to continue."
                            }
                        },
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

    fn speech_command(&mut self, command: speech::Command) -> bool {
        if self
            .speech
            .as_ref()
            .is_some_and(|session| session.commands.send(command).is_ok())
        {
            return true;
        }
        self.speech_input = false;
        self.speech_output = false;
        self.cancel_microphone();
        self.speaking = false;
        set_text(
            self.speech_status,
            "Speech worker stopped. Restart the demo to retry.",
        );
        false
    }

    fn stop_speech(&mut self) {
        self.cancel_microphone();
        if let Some(session) = &self.speech {
            let _result = session.commands.send(speech::Command::StopSpeaking);
        }
        self.listening = false;
        self.dictation_requested = false;
        self.speaking = false;
    }

    fn cancel_microphone(&mut self) {
        if let Some(session) = &self.speech {
            session.set_input_generation(None);
            let _result = session.commands.send(speech::Command::StopListening);
        }
        if let Some(microphone) = &mut self.microphone {
            microphone.stop();
        }
        self.microphone_generation = None;
        self.dictation_requested = false;
        self.listening = false;
    }

    fn toggle_dictation(&mut self) {
        if self.dictation_requested {
            self.cancel_microphone();
            set_text(
                self.speech_status,
                "Microphone disabled. Review your message before sending.",
            );
        } else if !self.speech_input {
            self.speech_help("Microphone setup", self.microphone_help());
        } else if !self.ready {
            set_text(
                self.speech_status,
                "Swift is not ready yet. Enable the microphone after model startup completes.",
            );
        } else if !self.busy && !self.speaking {
            let Some(microphone) = &mut self.microphone else {
                return;
            };
            match microphone.start() {
                Ok(generation) => {
                    if let Some(session) = &self.speech {
                        session.set_input_generation(Some(generation));
                    }
                    self.microphone_generation = Some(generation);
                    self.dictation_requested = true;
                    set_text(
                        self.speech_status,
                        "Requesting Windows microphone permission...",
                    );
                    // SAFETY: The user explicitly enabled input for this live native edit.
                    unsafe { SetFocus(self.input) };
                }
                Err(_) => set_text(
                    self.speech_status,
                    "Windows microphone permission is unavailable. Check privacy settings and app capabilities.",
                ),
            }
        }
        self.update_buttons();
    }

    fn toggle_read_aloud(&mut self) {
        if self.speaking {
            if self.speech_command(speech::Command::StopSpeaking) {
                self.speaking = false;
                set_text(self.speech_status, "Read aloud stopped.");
            }
        } else if !self.speech_output {
            self.speech_help("Read aloud setup", self.read_aloud_help());
        } else if !self.answer_complete || self.last_answer.is_empty() {
            set_text(
                self.speech_status,
                "Read aloud needs a completed answer. Send a message to Swift first.",
            );
        } else {
            let voice_description = control_text(self.voice_profile).trim().to_owned();
            if voice_description.is_empty() {
                set_text(
                    self.speech_status,
                    "Describe the voice in Voice profile before reading aloud.",
                );
                // SAFETY: This enabled native setting requires the user's description.
                unsafe { SetFocus(self.voice_profile) };
                return;
            }
            self.stop_speech();
            if self.speech_command(speech::Command::Speak {
                text: self.last_answer.clone(),
                voice_description,
            }) {
                self.speaking = true;
                set_text(self.speech_status, "Reading the visible answer aloud.");
            }
        }
        self.update_buttons();
    }

    const fn microphone_help(&self) -> &'static str {
        if !self.canary_configured {
            "Microphone recognition needs the Canary Qwen 2.5B runtime and service. The PowerShell demo script downloads its pinned model files automatically. Set up the runtime, start the service, then launch the demo with -CanaryKeyFile pointing to its protected key file. See tools/canary-asr/README.md. Windows permission is requested only when you enable a ready microphone; nothing is recording now."
        } else if self.microphone.is_none() {
            "Windows microphone access could not initialize. Check Windows microphone privacy settings and restart the demo. Nothing is recording."
        } else {
            "Canary Qwen 2.5B is not ready. Start its service on the configured port, check its protected key file, then restart the demo. See tools/canary-asr/README.md. Nothing is recording."
        }
    }

    const fn read_aloud_help(&self) -> &'static str {
        if self.tts_configured {
            "Qwen3-TTS VoiceDesign is not ready. Start its service on the configured port, check its protected key file, then restart the demo. See tools/qwen-tts/README.md. Your voice description will be used when the service is ready."
        } else {
            "Read aloud needs the Qwen3-TTS 1.7B VoiceDesign runtime and service. The PowerShell demo script downloads its pinned model files automatically. Set up the runtime, start the service, then launch the demo with -TtsKeyFile pointing to its protected key file. See tools/qwen-tts/README.md. Describe your desired voice in Voice profile; no preset or reference recording is required."
        }
    }

    fn speech_help(&self, title: &str, message: &str) {
        set_text(self.speech_status, message);
        let title = wide(title);
        let message = wide(message);
        // SAFETY: This UI-thread window owns the modal native help dialog; both
        // NUL-terminated strings live until the synchronous call returns.
        unsafe {
            MessageBoxW(
                self.window,
                message.as_ptr(),
                title.as_ptr(),
                MB_OK | MB_ICONINFORMATION,
            )
        };
    }

    fn drain_speech_events(&mut self) {
        for _ in 0..32 {
            let Some(session) = &self.speech else {
                break;
            };
            match session.events.try_recv() {
                Ok(event) => self.speech_event(event),
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.speech_input = false;
                    self.speech_output = false;
                    self.cancel_microphone();
                    self.speaking = false;
                    set_text(
                        self.speech_status,
                        "Speech unavailable. Text chat remains available.",
                    );
                    self.update_buttons();
                    break;
                }
            }
        }
    }

    fn speech_event(&mut self, event: speech::Event) {
        match event {
            speech::Event::Capabilities { input, output } => {
                self.speech_input = input && self.microphone.is_some();
                self.speech_output = output;
                set_text(
                    self.speech_status,
                    match (self.speech_input, output) {
                        (true, true) => "Microphone ready. Read aloud needs a completed answer.",
                        (true, false) => "Microphone ready. Select Read aloud for Qwen setup.",
                        (false, true) => "Qwen ready. Select Microphone for input setup.",
                        (false, false) => {
                            "Speech setup required. Select Microphone or Read aloud for instructions."
                        }
                    },
                );
            }
            speech::Event::Listening(listening) => {
                self.listening = listening && self.dictation_requested;
                set_text(
                    self.speech_status,
                    if self.listening {
                        "Recording. Review transcribed text before sending."
                    } else if self.dictation_requested {
                        "Microphone enabled."
                    } else {
                        "Microphone disabled."
                    },
                );
            }
            speech::Event::Recognized { generation, text } => {
                self.accept_transcript(generation, &text);
            }
            speech::Event::Speaking {
                generation,
                speaking,
            } => self.output_status(
                generation,
                speaking,
                if speaking {
                    "Reading the visible answer aloud."
                } else {
                    "Read aloud stopped."
                },
            ),
            speech::Event::Failed {
                generation,
                message,
            } => self.output_status(generation, false, message),
            speech::Event::InputFailed {
                generation,
                message,
            } => {
                if self.dictation_requested && self.microphone_generation == Some(generation) {
                    self.cancel_microphone();
                    set_text(self.speech_status, message);
                }
            }
            speech::Event::Status(message) if self.dictation_requested => {
                set_text(self.speech_status, message);
            }
            speech::Event::Status(_) => {}
        }
        self.update_buttons();
    }

    fn output_status(&mut self, generation: u64, speaking: bool, message: &str) {
        if self
            .speech
            .as_ref()
            .is_some_and(|session| session.is_current_output(generation))
        {
            self.speaking = speaking;
            set_text(self.speech_status, message);
        }
    }

    fn accept_transcript(&mut self, generation: u64, text: &str) {
        if !self.dictation_requested
            || self.microphone_generation != Some(generation)
            || !self.ready
            || self.busy
        {
            return;
        }
        if !text.trim().is_empty() {
            let phrase = wide(&format!("{} ", text.trim()));
            // SAFETY: The live edit copies this bounded phrase synchronously; its
            // configured text limit still applies. Transcription never sends chat.
            unsafe {
                SendMessageW(
                    self.input,
                    EM_REPLACESEL,
                    1,
                    phrase.as_ptr().addr().cast_signed(),
                )
            };
        }
        if self
            .microphone
            .as_mut()
            .is_some_and(|microphone| microphone.resume(generation).is_err())
        {
            self.cancel_microphone();
            set_text(
                self.speech_status,
                "Microphone capture became unavailable. Enable it again to retry.",
            );
        } else {
            set_text(
                self.speech_status,
                "Transcript ready to review. Resuming microphone recording...",
            );
        }
    }

    fn poll_microphone(&mut self) {
        let Some(microphone) = &mut self.microphone else {
            return;
        };
        for event in microphone.poll() {
            match event {
                microphone::Event::Status(message) if self.dictation_requested => {
                    set_text(self.speech_status, message);
                }
                microphone::Event::Status(_) => {}
                microphone::Event::Failed(message) => {
                    self.cancel_microphone();
                    set_text(self.speech_status, message);
                }
                microphone::Event::Clip { generation, wav } => {
                    if self.dictation_requested && self.microphone_generation == Some(generation) {
                        self.listening = false;
                        if self.speech_command(speech::Command::Transcribe { generation, wav }) {
                            set_text(
                                self.speech_status,
                                "Transcribing locally. Microphone capture is paused.",
                            );
                        }
                    }
                }
            }
        }
        self.update_buttons();
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
            bounds.ptMinTrackSize.y = 800_i32.saturating_mul(dpi) / 96;
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
            if lparam != 0 {
                // SAFETY: WM_DPICHANGED supplies a message-scoped suggested RECT.
                let suggested = unsafe { *without_provenance_mut::<RECT>(lparam.cast_unsigned()) };
                // SAFETY: The owned HWND and scalar screen coordinates are valid.
                // Reentrant size messages use try_borrow_mut; layout runs again below.
                unsafe {
                    SetWindowPos(
                        window,
                        null_mut(),
                        suggested.left,
                        suggested.top,
                        suggested.right.saturating_sub(suggested.left),
                        suggested.bottom.saturating_sub(suggested.top),
                        SWP_NOACTIVATE | SWP_NOZORDER,
                    );
                }
            }
            state.set_font();
            state.layout();
            true
        }
        WM_SETTINGCHANGE | WM_SYSCOLORCHANGE | WM_THEMECHANGED => {
            state.refresh_theme();
            true
        }
        WM_TIMER if wparam == TIMER => {
            state.drain_events();
            state.poll_microphone();
            state.drain_speech_events();
            true
        }
        WM_COMMAND => {
            match wparam & 0xffff {
                SEND => state.submit(),
                DICTATE => state.toggle_dictation(),
                READ_ALOUD => state.toggle_read_aloud(),
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
                    state.stop_speech();
                    state.last_answer.clear();
                    state.answer_complete = false;
                    let _result = state.commands.send(Command::NewChat);
                    transcript::welcome(state.transcript);
                    state.chat_started = false;
                    set_text(state.reasoning, "");
                    state.update_buttons();
                    // SAFETY: The composer is an enabled live control on this thread.
                    unsafe { SetFocus(state.input) };
                }
                THINKING => {
                    state.show_thinking = !state.show_thinking;
                    set_text(
                        state.thinking,
                        if state.show_thinking {
                            "Hide t&hinking"
                        } else {
                            "Show t&hinking"
                        },
                    );
                    state.layout();
                }
                _ => {}
            }
            true
        }
        WM_CLOSE => {
            state.stop_speech();
            state.microphone.take();
            state.speech.take();
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
    set_dark_frame(window, !colors().high_contrast);
    let corners: u32 = 2;
    // SAFETY: Attribute 33 takes a four-byte corner preference, alive through the synchronous call.
    unsafe {
        DwmSetWindowAttribute(window, 33, (&raw const corners).cast(), 4);
    }
    Ok(window)
}

fn set_dark_frame(window: HWND, enabled: bool) {
    let dark = i32::from(enabled);
    // SAFETY: DWMWA_USE_IMMERSIVE_DARK_MODE takes this four-byte BOOL synchronously.
    // Older Windows versions can harmlessly decline this appearance hint.
    unsafe {
        DwmSetWindowAttribute(window, 20, (&raw const dark).cast(), 4);
    }
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
    let microphone = microphone::Microphone::new().ok();
    let mut state = State::create(
        window,
        commands.clone(),
        incoming_events,
        &config,
        microphone,
    )?;
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
