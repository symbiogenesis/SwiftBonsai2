//! Passive native-control checks: no permission requests, capture or playback.

use super::*;
use windows_sys::Win32::UI::{
    Input::KeyboardAndMouse::IsWindowEnabled,
    WindowsAndMessaging::{GWL_STYLE, GetWindowLongPtrW, GetWindowRect, IsWindowVisible},
};

struct HiddenWindow(HWND);
impl HiddenWindow {
    fn create() -> io::Result<Self> {
        // SAFETY: STATIC is a system class and all creation inputs are scalar or
        // static. Omitting WS_VISIBLE keeps this owned test window hidden.
        let window = unsafe {
            CreateWindowExW(
                0,
                w!("STATIC"),
                w!("Speech controls regression"),
                WS_OVERLAPPEDWINDOW,
                0,
                0,
                1100,
                870,
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
impl Drop for HiddenWindow {
    fn drop(&mut self) {
        // SAFETY: The test owns this thread's hidden parent and its children.
        unsafe { DestroyWindow(self.0) };
    }
}

fn enabled(control: HWND) -> bool {
    // SAFETY: The test queries a live native child on its creating UI thread.
    unsafe { IsWindowEnabled(control) != 0 }
}

fn bounds(control: HWND) -> io::Result<RECT> {
    let mut bounds = RECT::default();
    // SAFETY: Live HWND and writable correctly-sized rectangle storage.
    if unsafe { GetWindowRect(control, &raw mut bounds) } == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(bounds)
    }
}

#[test]
fn speech_controls_stay_visible_and_explain_unconfigured_services_without_listening()
-> io::Result<()> {
    let _rich_edit = transcript::Library::load()?;
    let window = HiddenWindow::create()?;
    let (commands, _incoming_commands) = mpsc::channel();
    let (_events, incoming_events) = mpsc::channel();
    let temporary = std::env::temp_dir();
    let config = Config {
        broker: temporary.join("unused-broker.exe"),
        data: temporary.clone(),
        evidence: temporary,
        frontend_port: 5173,
        backend_port: 8081,
        use_abliterate_lora: true,
        canary: None,
        qwen_tts: None,
    };
    // No microphone controller is created; services are unconfigured and the
    // chat worker is never started. This test cannot request or capture audio.
    let mut state = State::create(window.0, commands, incoming_events, &config, None)?;
    state.layout();
    state.update_buttons();
    assert!(!state.dictation_requested);
    assert!(state.microphone_generation.is_none());
    assert!(state.microphone.is_none());
    assert_eq!(control_text(state.dictate), "&Microphone: off");
    assert!(enabled(state.dictate));
    assert!(enabled(state.read_aloud));
    assert!(enabled(state.voice_profile));
    assert_eq!(control_text(state.voice_profile), DEFAULT_VOICE);
    assert!(state.microphone_help().contains("-CanaryKeyFile"));
    assert!(state.read_aloud_help().contains("Start-Demo.ps1"));
    assert!(state.read_aloud_help().contains("-NoTts"));
    state.speech_event(speech::Event::Capabilities {
        input: false,
        output: false,
    });
    assert!(control_text(state.speech_status).contains("Speech setup required"));
    assert!(enabled(state.dictate));
    assert!(enabled(state.read_aloud));
    // The child keeps WS_VISIBLE even though the parent remains hidden.
    // SAFETY: Both style and visibility queries address this test's live HWNDs.
    let style = unsafe { GetWindowLongPtrW(state.dictate, GWL_STYLE) }.cast_unsigned();
    assert_ne!(style & usize::try_from(WS_VISIBLE).unwrap_or_default(), 0);
    // SAFETY: The parent is a valid owned hidden native window.
    assert_eq!(unsafe { IsWindowVisible(window.0) }, 0);
    let parent = bounds(window.0)?;
    let microphone = bounds(state.dictate)?;
    let read_aloud = bounds(state.read_aloud)?;
    assert!(microphone.left >= parent.left && microphone.right <= parent.right);
    assert!(microphone.top >= parent.top && microphone.bottom <= parent.bottom);
    assert!(microphone.right < read_aloud.left);
    let speech = state
        .speech
        .as_ref()
        .ok_or_else(|| io::Error::other("Speech worker did not start."))?;
    speech
        .commands
        .send(speech::Command::StopSpeaking)
        .map_err(io::Error::other)?;
    let status = control_text(state.speech_status);
    state.speech_event(speech::Event::Speaking {
        generation: 0,
        speaking: true,
    });
    state.speech_event(speech::Event::Failed {
        generation: 0,
        message: "stale output failure",
    });
    assert!(!state.speaking);
    assert_eq!(control_text(state.speech_status), status);
    state.speech_output = true;
    state.toggle_read_aloud();
    assert!(control_text(state.speech_status).contains("completed answer"));
    state.speaking = true;
    state.update_buttons();
    assert!(!enabled(state.dictate));
    assert!(!enabled(state.voice_profile));
    assert!(enabled(state.read_aloud));
    state.speaking = false;
    state.busy = true;
    state.update_buttons();
    assert!(!enabled(state.dictate));
    assert!(!enabled(state.read_aloud));
    Ok(())
}

fn response_state(window: HWND) -> io::Result<(State, Receiver<Command>, Sender<Event>)> {
    let (commands, incoming_commands) = mpsc::channel();
    let (events, incoming_events) = mpsc::channel();
    let temporary = std::env::temp_dir();
    let config = Config {
        broker: temporary.join("unused-broker.exe"),
        data: temporary.clone(),
        evidence: temporary,
        frontend_port: 5173,
        backend_port: 8081,
        use_abliterate_lora: true,
        canary: None,
        qwen_tts: None,
    };
    Ok((
        State::create(window, commands, incoming_events, &config, None)?,
        incoming_commands,
        events,
    ))
}

#[test]
fn response_controls_preserve_limits_recover_composer_and_retain_complete_draft() -> io::Result<()>
{
    let _rich_edit = transcript::Library::load()?;
    let window = HiddenWindow::create()?;
    let (mut state, commands, events) = response_state(window.0)?;
    state.ready = true;
    state.layout();
    state.update_buttons();
    assert_eq!(selected(state.mode), 0);
    assert_eq!(selected(state.output_budget), 3);
    assert!(control_text(state.context_hint).contains("8,192"));
    assert!(control_text(state.context_hint).contains("thinking"));
    assert!(control_text(state.context_hint).contains("reserve"));
    assert!(control_text(state.mode_hint).contains("unproven"));
    // SAFETY: The test queries a live dropdown's scalar native window style.
    let mode_style = unsafe { GetWindowLongPtrW(state.mode, GWL_STYLE) }.cast_unsigned();
    assert_ne!(
        mode_style & usize::try_from(WS_TABSTOP).unwrap_or_default(),
        0
    );
    set_text(state.input, "  Original task  ");
    state.submit();
    assert!(matches!(commands.try_recv(), Ok(Command::Send {
        text,
        max_completion_tokens: 4096,
        extra_effort: false,
    }) if text == "Original task"));
    assert!(!enabled(state.mode));
    assert!(!enabled(state.output_budget));
    events
        .send(Event::Rejected {
            message: "Choose a shorter task or a lower output cap.".to_owned(),
            retry_required: false,
            text: "Original task".to_owned(),
        })
        .map_err(io::Error::other)?;
    state.drain_events();
    assert_eq!(control_text(state.input), "Original task");
    assert!(state.ready && !state.busy);
    assert!(enabled(state.send) && enabled(state.new_chat));
    assert!(enabled(state.mode) && enabled(state.output_budget));
    // SAFETY: These valid dropdown indices select the explicit opt-in and 512 cap.
    unsafe { SendMessageW(state.mode, CB_SETCURSEL, 1, 0) };
    // SAFETY: Same scalar dropdown-selection contract as above.
    unsafe { SendMessageW(state.output_budget, CB_SETCURSEL, 0, 0) };
    state.submit();
    assert!(matches!(commands.try_recv(), Ok(Command::Send {
        text,
        max_completion_tokens: 512,
        extra_effort: true,
    }) if text == "Original task"));
    events
        .send(Event::Stage("Extra effort: Drafting (2/3)"))
        .map_err(io::Error::other)?;
    events
        .send(Event::Text("The complete draft.".to_owned()))
        .map_err(io::Error::other)?;
    events
        .send(Event::Draft("The complete draft.".to_owned()))
        .map_err(io::Error::other)?;
    events
        .send(Event::Stage("Extra effort: Reviewing (3/3)"))
        .map_err(io::Error::other)?;
    events
        .send(Event::Text("Partial revision".to_owned()))
        .map_err(io::Error::other)?;
    state.drain_events();
    assert_eq!(state.last_answer, "Partial revision");
    assert_eq!(control_text(state.status), "Extra effort: Reviewing (3/3)");
    state.stop_generation();
    assert!(matches!(commands.try_recv(), Ok(Command::Stop)));
    assert!(!state.ready && !state.busy && !state.retryable);
    assert!(!enabled(state.retry));
    events
        .send(Event::Rejected {
            message: "Stopped.".to_owned(),
            retry_required: true,
            text: "Original task".to_owned(),
        })
        .map_err(io::Error::other)?;
    state.drain_events();
    assert!(!state.ready && !enabled(state.send) && !enabled(state.new_chat));
    assert!(!state.retryable && !enabled(state.retry));
    events
        .send(Event::Retryable(
            "Generation stopped. Select Retry model to continue.".to_owned(),
        ))
        .map_err(io::Error::other)?;
    state.drain_events();
    assert!(state.retryable && !state.ready);
    assert!(enabled(state.retry));
    assert!(!enabled(state.send));
    assert_eq!(control_text(state.input), "Original task");
    assert_eq!(state.last_answer, "The complete draft.");
    assert!(state.answer_complete);
    assert!(!control_text(state.status).contains("memory"));
    assert!(!state.dictation_requested && state.microphone_generation.is_none());
    assert!(state.microphone.is_none());
    Ok(())
}

#[test]
fn response_controls_fit_the_minimum_window_without_overlapping_conversation() -> io::Result<()> {
    let _rich_edit = transcript::Library::load()?;
    let window = HiddenWindow::create()?;
    let (mut state, _commands, _events) = response_state(window.0)?;
    // SAFETY: Live hidden window queried only for its DPI value.
    let dpi = i32::try_from(unsafe { GetDpiForWindow(window.0) }.max(96)).unwrap_or(96);
    // SAFETY: This test owns the hidden window and uses the real minimum outer size.
    unsafe { MoveWindow(window.0, 0, 0, 680 * dpi / 96, 800 * dpi / 96, 0) };
    state.show_thinking = true;
    state.layout();
    let parent = bounds(window.0)?;
    for control in [
        state.mode,
        state.output_budget,
        state.context_hint,
        state.transcript,
        state.reasoning,
    ] {
        let child = bounds(control)?;
        assert!(child.left >= parent.left && child.right <= parent.right);
        assert!(child.top >= parent.top && child.bottom <= parent.bottom);
        assert!(child.right > child.left && child.bottom > child.top);
    }
    assert!(bounds(state.mode)?.right < bounds(state.budget_label)?.left);
    assert!(bounds(state.output_budget)?.bottom < bounds(state.mode_hint)?.top);
    assert!(bounds(state.context_hint)?.bottom < bounds(state.transcript_label)?.top);
    assert!(bounds(state.transcript)?.bottom < bounds(state.reasoning_label)?.top);
    assert!(bounds(state.reasoning)?.bottom < bounds(state.input_label)?.top);
    assert!(!state.dictation_requested && state.microphone.is_none());
    Ok(())
}
