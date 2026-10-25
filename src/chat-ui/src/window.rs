//! Win32 controls and message dispatch, confined to the main UI thread.
//!
//! Control handles never cross threads. Reentrant messages use `try_borrow_mut`
//! rather than producing overlapping references to the window state.
#![allow(
    clippy::redundant_pub_crate,
    reason = "Keep the audited native boundary explicitly parent-only."
)]

use crate::client::{self, Command, Config, Event};
use std::{
    cell::RefCell,
    io,
    ptr::{null, null_mut, without_provenance_mut},
    sync::mpsc::{self, Receiver, Sender},
};
use windows_sys::{
    Win32::{
        Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM},
        Graphics::Gdi::{
            CLEARTYPE_QUALITY, CLIP_DEFAULT_PRECIS, COLOR_WINDOW, CreateFontW, DEFAULT_CHARSET,
            DEFAULT_PITCH, DeleteObject, FW_NORMAL, GetSysColorBrush, HFONT, OUT_DEFAULT_PRECIS,
        },
        System::LibraryLoader::GetModuleHandleW,
        UI::{
            Controls::{EM_REPLACESEL, EM_SCROLLCARET, EM_SETLIMITTEXT, EM_SETSEL},
            HiDpi::{
                DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, GetDpiForWindow,
                SetProcessDpiAwarenessContext,
            },
            Input::KeyboardAndMouse::{
                EnableWindow, GetFocus, GetKeyState, SetFocus, VK_RETURN, VK_SHIFT,
            },
            WindowsAndMessaging::{
                BS_DEFPUSHBUTTON, CW_USEDEFAULT, CreateWindowExW, DefWindowProcW, DestroyWindow,
                DispatchMessageW, ES_AUTOVSCROLL, ES_MULTILINE, ES_READONLY, ES_WANTRETURN,
                GetClientRect, GetMessageW, GetWindowTextLengthW, GetWindowTextW, IDC_ARROW,
                IDI_APPLICATION, IsDialogMessageW, KillTimer, LoadCursorW, LoadIconW, MB_ICONERROR,
                MB_OK, MSG, MessageBoxW, MoveWindow, PostQuitMessage, RegisterClassW, SW_HIDE,
                SW_SHOW, SendMessageW, SetTimer, SetWindowTextW, ShowWindow, TranslateMessage,
                WM_CLOSE, WM_COMMAND, WM_DESTROY, WM_DPICHANGED, WM_KEYDOWN, WM_SETFONT, WM_SIZE,
                WM_TIMER, WNDCLASSW, WS_CHILD, WS_EX_CLIENTEDGE, WS_EX_CONTROLPARENT,
                WS_OVERLAPPEDWINDOW, WS_TABSTOP, WS_VISIBLE, WS_VSCROLL,
            },
        },
    },
    core::w,
};

const SEND: usize = 101;
const STOP: usize = 102;
const NEW: usize = 103;
const THINKING: usize = 104;
const TIMER: usize = 1;

thread_local! {
    static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
}

#[allow(
    clippy::struct_excessive_bools,
    reason = "Separate native control flags track input, generation, panel visibility and retry availability."
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
    status: HWND,
    font: HFONT,
    commands: Sender<Command>,
    events: Receiver<Event>,
    ready: bool,
    busy: bool,
    show_thinking: bool,
    retryable: bool,
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
    let text = wide(&text.replace('\n', "\r\n"));
    // SAFETY: This is a live EDIT control on the UI thread. EM_SETSEL accepts
    // usize::MAX as the documented end-of-text sentinel, with no pointer input.
    unsafe {
        SendMessageW(control, EM_SETSEL, usize::MAX, -1);
    }
    // SAFETY: EM_REPLACESEL consumes the live NUL-terminated buffer synchronously.
    unsafe {
        SendMessageW(
            control,
            EM_REPLACESEL,
            0,
            text.as_ptr().addr().cast_signed(),
        );
    }
    // SAFETY: The live edit control accepts this pointer-free scrolling message.
    unsafe {
        SendMessageW(control, EM_SCROLLCARET, 0, 0);
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

fn edit(parent: HWND, readonly: bool) -> io::Result<HWND> {
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
            WS_EX_CLIENTEDGE,
            w!("EDIT"),
            w!(""),
            WS_CHILD | WS_VISIBLE | style,
            0,
            0,
            1,
            1,
            parent,
            null_mut(),
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
            title: control(window, w!("STATIC"), "Swift Bonsai 2", 0, 0)?,
            subtitle: control(
                window,
                w!("STATIC"),
                "Native Rust  /  Local CPU  /  Creative writing",
                0,
                0,
            )?,
            transcript: edit(window, true)?,
            reasoning: edit(window, true)?,
            input: edit(window, false)?,
            send: control(
                window,
                w!("BUTTON"),
                "Send",
                WS_TABSTOP | u32::try_from(BS_DEFPUSHBUTTON).unwrap_or_default(),
                SEND,
            )?,
            stop: control(window, w!("BUTTON"), "Stop", WS_TABSTOP, STOP)?,
            new_chat: control(window, w!("BUTTON"), "New conversation", WS_TABSTOP, NEW)?,
            thinking: control(window, w!("BUTTON"), "Show thinking", WS_TABSTOP, THINKING)?,
            status: control(
                window,
                w!("STATIC"),
                "Verifying model and starting Swift. This can take a moment...",
                0,
                0,
            )?,
            font: null_mut(),
            commands,
            events,
            ready: false,
            busy: false,
            show_thinking: false,
            retryable: false,
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
            EnableWindow(
                self.new_chat,
                i32::from((self.ready && !self.busy) || self.retryable),
            );
        }
        // SAFETY: Same control lifetime/thread contract as above.
        unsafe {
            EnableWindow(self.input, i32::from(self.ready && !self.busy));
        }
        // SAFETY: Same control lifetime/thread contract as above.
        unsafe {
            EnableWindow(self.stop, i32::from(self.busy));
        }
    }

    fn layout(&mut self) {
        let mut bounds = RECT::default();
        // SAFETY: window is live on its UI thread; bounds points to writable RECT storage.
        unsafe {
            GetClientRect(self.window, &raw mut bounds);
        }
        // SAFETY: window is a live HWND; no pointers are passed.
        let dpi = unsafe { GetDpiForWindow(self.window) }.max(96);
        let scale = |value: i32| value.saturating_mul(i32::try_from(dpi).unwrap_or(96)) / 96;
        let width = bounds.right.max(scale(480));
        let height = bounds.bottom.max(scale(400));
        let pad = scale(20);
        let content_width = width - 2 * pad;
        let top = scale(116);
        let bottom = height - scale(160);
        let split = if self.show_thinking {
            (bottom - top) / 3
        } else {
            0
        };
        let transcript_height = (bottom - top - split).max(scale(60));
        let place = |handle, x, y, w, h| {
            // SAFETY: Each supplied HWND is live on this thread; all dimensions
            // are bounded signed layout values; MoveWindow retains no references.
            unsafe {
                MoveWindow(handle, x, y, w, h, 1);
            }
        };
        place(self.title, pad, scale(16), content_width, scale(26));
        place(self.subtitle, pad, scale(46), content_width, scale(22));
        place(self.new_chat, pad, scale(78), scale(160), scale(28));
        place(
            self.thinking,
            width - pad - scale(140),
            scale(78),
            scale(140),
            scale(28),
        );
        place(self.transcript, pad, top, content_width, transcript_height);
        place(
            self.reasoning,
            pad,
            top + transcript_height + scale(6),
            content_width,
            (split - scale(6)).max(1),
        );
        // SAFETY: reasoning is a live child window; visibility is a scalar flag.
        unsafe {
            ShowWindow(
                self.reasoning,
                if self.show_thinking { SW_SHOW } else { SW_HIDE },
            );
        }
        place(
            self.input,
            pad,
            bottom + scale(12),
            content_width - scale(106),
            scale(92),
        );
        place(
            self.send,
            width - pad - scale(94),
            bottom + scale(12),
            scale(94),
            scale(40),
        );
        place(
            self.stop,
            width - pad - scale(94),
            bottom + scale(62),
            scale(94),
            scale(40),
        );
        place(
            self.status,
            pad,
            height - scale(34),
            content_width,
            scale(26),
        );
    }

    fn set_font(&mut self) {
        // SAFETY: window is a live HWND belonging to this thread.
        let dpi = unsafe { GetDpiForWindow(self.window) }.max(96);
        let height = -(18 * i32::try_from(dpi).unwrap_or(96) / 96);
        // SAFETY: CreateFontW consumes scalar font settings and a static
        // NUL-terminated face name; the returned GDI handle is owned by State.
        let font = unsafe {
            CreateFontW(
                height,
                0,
                0,
                0,
                FW_NORMAL.cast_signed(),
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
        };
        if font.is_null() {
            return;
        }
        for handle in [
            self.title,
            self.subtitle,
            self.transcript,
            self.reasoning,
            self.input,
            self.send,
            self.stop,
            self.new_chat,
            self.thinking,
            self.status,
        ] {
            // SAFETY: Control and font are live; WM_SETFONT synchronously stores
            // the GDI handle, which remains owned until replaced or controls die.
            unsafe {
                SendMessageW(handle, WM_SETFONT, font.addr(), 1);
            }
        }
        if !self.font.is_null() {
            // SAFETY: Every live control has just switched to the new font;
            // the old owned font is no longer selected in any control.
            unsafe {
                DeleteObject(self.font);
            }
        }
        self.font = font;
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
        append(
            self.transcript,
            &format!("\nYou\n{text}\n\nSwift Bonsai 2\n"),
        );
        set_text(self.input, "");
        set_text(self.reasoning, "");
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
        set_text(self.status, message);
        append(self.transcript, &format!("\n\nDemo status\n{message}\n"));
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
                    self.retryable = false;
                    set_text(self.new_chat, "New conversation");
                    set_text(self.status, "Verifying model and available memory...");
                    self.update_buttons();
                }
                Event::Retryable(message) => {
                    self.fail(&message);
                    self.retryable = true;
                    set_text(self.new_chat, "Retry model");
                    self.update_buttons();
                }
                Event::Ready => {
                    self.ready = true;
                    set_text(
                        self.status,
                        "Ready. Enter to send; Shift+Enter for a new line.",
                    );
                    self.update_buttons();
                    // SAFETY: input is an enabled live control on this thread.
                    unsafe {
                        SetFocus(self.input);
                    }
                }
                Event::Text(text) => {
                    append(self.transcript, &text);
                    set_text(self.status, "Writing...");
                }
                Event::Reasoning(text) => append(self.reasoning, &text),
                Event::Complete => {
                    self.busy = false;
                    append(self.transcript, "\n");
                    set_text(
                        self.status,
                        "Ready. Enter to send; Shift+Enter for a new line.",
                    );
                    self.update_buttons();
                    // SAFETY: input is an enabled live control on this thread.
                    unsafe {
                        SetFocus(self.input);
                    }
                }
                Event::Failed(message) => self.fail(&message),
            }
        }
    }
}

unsafe extern "system" fn procedure(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
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
                STOP if state.busy => {
                    let _result = state.commands.send(Command::Stop);
                    state.fail(
                        "Stopped. Close and restart the demo to start a fresh model session.",
                    );
                }
                NEW if state.retryable => {
                    state.retryable = false;
                    let _result = state.commands.send(Command::Retry);
                    state.update_buttons();
                }
                NEW if state.ready && !state.busy => {
                    let _result = state.commands.send(Command::NewChat);
                    set_text(
                        state.transcript,
                        "A new conversation. What would you like to create?\r\n",
                    );
                    set_text(state.reasoning, "");
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
    // SAFETY: Returns a shared system-owned brush; it must not be deleted.
    let brush = unsafe { GetSysColorBrush(COLOR_WINDOW) };
    let class = WNDCLASSW {
        lpfnWndProc: Some(procedure),
        hInstance: instance,
        hIcon: icon,
        hCursor: cursor,
        hbrBackground: brush,
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
            WS_OVERLAPPEDWINDOW,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            1000,
            820,
            null_mut(),
            null_mut(),
            instance,
            null(),
        )
    };
    if window.is_null() {
        return Err(io::Error::last_os_error());
    }
    Ok(window)
}

pub(super) fn run(config: Config) -> io::Result<()> {
    let window = create_window()?;
    let (commands, incoming_commands) = mpsc::channel();
    let (events, incoming_events) = mpsc::channel();
    let mut state = State::create(window, commands.clone(), incoming_events)?;
    state.set_font();
    state.layout();
    state.update_buttons();
    set_text(
        state.transcript,
        "Your local creative writing partner.\r\n\r\nTry: Write a short opening scene set in a quiet greenhouse at midnight.\r\n\r\nThe model is starting. Your conversation stays in memory.\r\n",
    );
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
        if let Some(state) = slot.borrow_mut().take()
            && !state.font.is_null()
        {
            // SAFETY: WM_DESTROY has destroyed every child control, so none
            // retains the owned font; delete it exactly once here.
            unsafe {
                DeleteObject(state.font);
            }
        }
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
