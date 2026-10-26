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
    assert!(state.read_aloud_help().contains("-TtsKeyFile"));
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
