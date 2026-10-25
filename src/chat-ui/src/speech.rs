//! Optional local Windows speech input and plain-text read-aloud output.
//!
//! All COM interfaces stay on one worker. Discovering installed tokens does not
//! open the microphone; only an explicit Listen command creates an input stream.
//! Recognized text returns to the composer for review, never directly to the model.

#![allow(
    clippy::redundant_pub_crate,
    reason = "The window uses this crate-private worker boundary."
)]

use std::{
    io,
    sync::mpsc::{self, Receiver, RecvTimeoutError, Sender},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};
use windows::{
    Win32::{
        Media::Speech::{
            ISpObjectToken, ISpObjectTokenCategory, ISpRecoContext, ISpRecoGrammar, ISpRecoResult,
            ISpRecognizer, ISpVoice, SPCAT_AUDIOIN, SPCAT_RECOGNIZERS, SPEI_RECOGNITION,
            SPET_LPARAM_IS_OBJECT, SPET_LPARAM_IS_POINTER, SPET_LPARAM_IS_STRING,
            SPET_LPARAM_IS_TOKEN, SPEVENT, SPF_ASYNC, SPF_IS_NOT_XML, SPF_PURGEBEFORESPEAK,
            SPLO_STATIC, SPRS_ACTIVE, SPRS_INACTIVE, SPRS_IS_SPEAKING, SPRST_INACTIVE_WITH_PURGE,
            SPVOICESTATUS, SpInprocRecognizer, SpObjectToken, SpObjectTokenCategory, SpVoice,
        },
        System::Com::{
            CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx,
            CoTaskMemFree, CoUninitialize,
        },
    },
    core::{IUnknown, Interface, PCWSTR, PWSTR},
};

const POLL: Duration = Duration::from_millis(100);
const LISTEN_LIMIT: Duration = Duration::from_secs(120);
const SPEAK_LIMIT: Duration = Duration::from_secs(600);
const MAX_TEXT: usize = 32768;

pub(crate) enum Command {
    Listen,
    StopListening,
    Speak(String),
    StopSpeaking,
    Shutdown,
}

pub(crate) enum Event {
    Capabilities { input: bool, output: bool },
    Listening(bool),
    Recognized(String),
    Speaking(bool),
    Failed(&'static str),
}

pub(crate) struct Session {
    pub(crate) commands: Sender<Command>,
    pub(crate) events: Receiver<Event>,
    worker: Option<JoinHandle<()>>,
}

impl Drop for Session {
    fn drop(&mut self) {
        let _sent = self.commands.send(Command::Shutdown);
        if let Some(worker) = self.worker.take() {
            let _joined = worker.join();
        }
    }
}

/// Start a worker without capturing audio or producing speech.
pub(crate) fn start() -> io::Result<Session> {
    let (commands, incoming) = mpsc::channel();
    let (outgoing, events) = mpsc::channel();
    let worker = thread::Builder::new()
        .name("bonsai-local-speech".to_owned())
        .spawn(move || run(&incoming, &outgoing))?;
    Ok(Session {
        commands,
        events,
        worker: Some(worker),
    })
}

struct Apartment;
impl Apartment {
    fn initialize() -> windows::core::Result<Self> {
        // SAFETY: This new worker initializes COM once and uses no interfaces
        // from other threads. The matching uninitialize occurs after their Drop.
        unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.ok()?;
        Ok(Self)
    }
}
impl Drop for Apartment {
    fn drop(&mut self) {
        // SAFETY: Balances this worker's successful CoInitializeEx; all local
        // COM interfaces have been released before the apartment guard drops.
        unsafe { CoUninitialize() };
    }
}

struct TaskText(PWSTR);
impl Drop for TaskText {
    fn drop(&mut self) {
        // SAFETY: SAPI returns CoTaskMem-allocated text; this owner frees it once.
        unsafe { CoTaskMemFree(Some(self.0.as_ptr().cast())) };
    }
}

fn default_token(category: PCWSTR) -> windows::core::Result<ISpObjectToken> {
    // SAFETY: A documented SAPI class is activated on this initialized worker.
    let tokens: ISpObjectTokenCategory =
        unsafe { CoCreateInstance(&SpObjectTokenCategory, None, CLSCTX_INPROC_SERVER) }?;
    // SAFETY: category is a static Windows SDK category ID; no registry mutation.
    unsafe { tokens.SetId(category, false) }?;
    // SAFETY: Valid same-thread category returns one owned CoTaskMem string.
    let id = TaskText(unsafe { tokens.GetDefaultTokenId() }?);
    // SAFETY: Documented token class, no aggregation or out-of-process activation.
    let token: ISpObjectToken =
        unsafe { CoCreateInstance(&SpObjectToken, None, CLSCTX_INPROC_SERVER) }?;
    // SAFETY: The null-terminated token ID remains live through this call; false
    // prevents creating or changing the user's speech configuration.
    unsafe { token.SetId(PCWSTR::null(), PCWSTR(id.0.as_ptr()), false) }?;
    Ok(token)
}

fn voice() -> windows::core::Result<ISpVoice> {
    // SAFETY: Documented Windows SAPI class on this initialized worker.
    let voice: ISpVoice = unsafe { CoCreateInstance(&SpVoice, None, CLSCTX_INPROC_SERVER) }?;
    // SAFETY: Discover an installed voice without synthesizing or playing audio.
    let _token = unsafe { voice.GetVoice() }?;
    Ok(voice)
}

struct Recognition {
    grammar: ISpRecoGrammar,
    context: ISpRecoContext,
    recognizer: ISpRecognizer,
    until: Instant,
}
impl Recognition {
    fn start() -> windows::core::Result<Self> {
        let audio = default_token(SPCAT_AUDIOIN)?;
        // SAFETY: A private in-process recognizer is created only after Listen;
        // its interfaces and input stay on this worker until Stop or shutdown.
        let recognizer: ISpRecognizer =
            unsafe { CoCreateInstance(&SpInprocRecognizer, None, CLSCTX_INPROC_SERVER) }?;
        // SAFETY: Documented inactive state before connecting the audio device.
        unsafe { recognizer.SetRecoState(SPRST_INACTIVE_WITH_PURGE) }?;
        // SAFETY: The installed default audio token is valid for this call.
        unsafe { recognizer.SetInput(&audio, true) }?;
        // SAFETY: Create a private context from a live same-thread recognizer.
        let context = unsafe { recognizer.CreateRecoContext() }?;
        // SPFEI includes SAPI's two flag-check bits, as defined in sapi.h.
        let interest = (1_u64 << SPEI_RECOGNITION.0) | (1_u64 << 30) | (1_u64 << 33);
        // SAFETY: Only completed recognition events are queued for this context.
        unsafe { context.SetInterest(interest, interest) }?;
        // SAFETY: Native event notification; no application callback or pointer.
        unsafe { context.SetNotifyWin32Event() }?;
        // SAFETY: Private grammar ID has no external interpretation.
        let grammar = unsafe { context.CreateGrammar(1) }?;
        // SAFETY: Null topic selects installed dictation, without custom files.
        unsafe { grammar.LoadDictation(PCWSTR::null(), SPLO_STATIC) }?;
        // SAFETY: The user's explicit Listen activates this private grammar.
        unsafe { grammar.SetDictationState(SPRS_ACTIVE) }?;
        // SAFETY: Start the recognizer only after the context and grammar exist.
        unsafe { recognizer.SetRecoState(windows::Win32::Media::Speech::SPRST_ACTIVE) }?;
        Ok(Self {
            grammar,
            context,
            recognizer,
            until: Instant::now() + LISTEN_LIMIT,
        })
    }

    fn poll(&self, events: &Sender<Event>) -> windows::core::Result<()> {
        // Bound work per iteration so a stream of events cannot starve Stop.
        for _ in 0..8 {
            let mut event = SPEVENT::default();
            let mut fetched = 0;
            // SAFETY: SAPI writes at most one correctly sized event and count.
            unsafe { self.context.GetEvents(1, &raw mut event, &raw mut fetched) }?;
            if fetched == 0 {
                break;
            }
            if recognition_event(&event) {
                // SAFETY: SPEI_RECOGNITION with object lParam transfers one
                // ISpRecoResult reference to us. from_raw consumes that reference.
                let result = unsafe {
                    ISpRecoResult::from_raw(std::ptr::without_provenance_mut(
                        event.lParam.0.cast_unsigned(),
                    ))
                };
                let mut pointer = PWSTR::null();
                // SAFETY: Valid result writes owned CoTaskMem UTF-16 text;
                // u32::MAX is SAPI's SP_GETWHOLEPHRASE constant for both indices.
                unsafe { result.GetText(u32::MAX, u32::MAX, true, &raw mut pointer, None) }?;
                let text = TaskText(pointer);
                if let Some(text) = phrase(&text)? {
                    let _sent = events.send(Event::Recognized(text));
                }
            } else {
                release_event(event);
            }
        }
        Ok(())
    }
}
impl Drop for Recognition {
    fn drop(&mut self) {
        // SAFETY: Deactivate this worker's live grammar before releasing it.
        let _stopped = unsafe { self.grammar.SetDictationState(SPRS_INACTIVE) };
        // SAFETY: Purge private recognition and release input when interfaces drop.
        let _stopped = unsafe { self.recognizer.SetRecoState(SPRST_INACTIVE_WITH_PURGE) };
    }
}

fn recognition_event(event: &SPEVENT) -> bool {
    let bits = event._bitfield.cast_unsigned();
    bits & 0xffff == SPEI_RECOGNITION.0.cast_unsigned()
        && bits >> 16 == SPET_LPARAM_IS_OBJECT.0.cast_unsigned()
        && event.lParam.0 != 0
}

fn release_event(event: SPEVENT) {
    if event.lParam.0 == 0 {
        return;
    }
    let kind = event._bitfield.cast_unsigned() >> 16;
    if kind == SPET_LPARAM_IS_OBJECT.0.cast_unsigned()
        || kind == SPET_LPARAM_IS_TOKEN.0.cast_unsigned()
    {
        // SAFETY: SAPI's object/token event contract transfers one IUnknown
        // reference; this temporary owner releases it exactly once.
        drop(unsafe {
            IUnknown::from_raw(std::ptr::without_provenance_mut(
                event.lParam.0.cast_unsigned(),
            ))
        });
    } else if kind == SPET_LPARAM_IS_POINTER.0.cast_unsigned()
        || kind == SPET_LPARAM_IS_STRING.0.cast_unsigned()
    {
        // SAFETY: SAPI's pointer/string event contract transfers CoTaskMem storage.
        unsafe {
            CoTaskMemFree(Some(std::ptr::without_provenance(
                event.lParam.0.cast_unsigned(),
            )))
        };
    }
}

fn phrase(text: &TaskText) -> windows::core::Result<Option<String>> {
    if text.0.is_null() {
        return Ok(None);
    }
    for length in 0..=MAX_TEXT {
        // SAFETY: SAPI guarantees a NUL-terminated allocation. Stop at its first
        // NUL; the bounded scan never continues beyond that terminator.
        if unsafe { *text.0.as_ptr().add(length) } == 0 {
            // SAFETY: The scan established the initialized UTF-16 prefix length.
            let units = unsafe { std::slice::from_raw_parts(text.0.as_ptr(), length) };
            return Ok((!units.is_empty()).then(|| String::from_utf16_lossy(units)));
        }
    }
    Err(windows::core::Error::from_hresult(windows::core::HRESULT(
        0x8007_0057_u32.cast_signed(),
    )))
}

fn speak(voice: &ISpVoice, text: &str) -> bool {
    let Some(text) = speech_text(text) else {
        return false;
    };
    let flags = (SPF_ASYNC | SPF_PURGEBEFORESPEAK | SPF_IS_NOT_XML)
        .0
        .cast_unsigned();
    // SAFETY: SAPI consumes the live NUL-terminated buffer during Speak. ASYNC
    // owns any subsequent synthesis; IS_NOT_XML prevents interpreting model text
    // as SSML, file references, or speech commands.
    unsafe { voice.Speak(PCWSTR(text.as_ptr()), flags, None) }.is_ok()
}

fn speech_text(text: &str) -> Option<Vec<u16>> {
    if text.trim().is_empty() || text.contains('\0') {
        return None;
    }
    let units: Vec<_> = text.encode_utf16().take(MAX_TEXT + 1).collect();
    (units.len() <= MAX_TEXT).then(|| units.into_iter().chain(Some(0)).collect())
}

fn stop_speaking(voice: Option<&ISpVoice>) {
    if let Some(voice) = voice {
        let flags = (SPF_ASYNC | SPF_PURGEBEFORESPEAK | SPF_IS_NOT_XML)
            .0
            .cast_unsigned();
        // SAFETY: Null text plus purge cancels this voice's queued speech.
        let _stopped = unsafe { voice.Speak(PCWSTR::null(), flags, None) };
    }
}

fn speaking(voice: &ISpVoice) -> bool {
    let mut status = SPVOICESTATUS::default();
    // SAFETY: Valid voice writes one SDK-layout status; null bookmark requests
    // no allocation. This does not start playback.
    unsafe { voice.GetStatus(&raw mut status, std::ptr::null_mut()) }.is_ok()
        && status.dwRunningState & SPRS_IS_SPEAKING.0.cast_unsigned() != 0
}

fn run(commands: &Receiver<Command>, events: &Sender<Event>) {
    let Ok(_apartment) = Apartment::initialize() else {
        let _sent = events.send(Event::Capabilities {
            input: false,
            output: false,
        });
        return;
    };
    let voice = voice().ok();
    let input = default_token(SPCAT_RECOGNIZERS).is_ok() && default_token(SPCAT_AUDIOIN).is_ok();
    let _sent = events.send(Event::Capabilities {
        input,
        output: voice.is_some(),
    });
    let mut recognition: Option<Recognition> = None;
    let mut speech_until: Option<Instant> = None;
    loop {
        match commands.recv_timeout(POLL) {
            Ok(Command::Shutdown) | Err(RecvTimeoutError::Disconnected) => break,
            Ok(Command::Listen) => {
                stop_speaking(voice.as_ref());
                speech_until = None;
                let _sent = events.send(Event::Speaking(false));
                recognition = Recognition::start().ok();
                let _sent = events.send(Event::Listening(recognition.is_some()));
                if recognition.is_none() {
                    let _sent = events.send(Event::Failed("Local dictation is unavailable. Check the installed Windows speech language, microphone, and privacy settings."));
                }
            }
            Ok(Command::StopListening) => {
                recognition = None;
                let _sent = events.send(Event::Listening(false));
            }
            Ok(Command::Speak(text)) => {
                recognition = None;
                let _sent = events.send(Event::Listening(false));
                if voice.as_ref().is_some_and(|voice| speak(voice, &text)) {
                    speech_until = Some(Instant::now() + SPEAK_LIMIT);
                    let _sent = events.send(Event::Speaking(true));
                } else {
                    stop_speaking(voice.as_ref());
                    speech_until = None;
                    let _sent = events.send(Event::Speaking(false));
                    let _sent = events.send(Event::Failed("Could not read this answer aloud. Check Windows voices and audio output; read-aloud accepts up to 32,768 UTF-16 characters."));
                }
            }
            Ok(Command::StopSpeaking) => {
                stop_speaking(voice.as_ref());
                speech_until = None;
                let _sent = events.send(Event::Speaking(false));
            }
            Err(RecvTimeoutError::Timeout) => {}
        }
        if let Some(active) = &recognition {
            let expired = Instant::now() >= active.until;
            if expired || active.poll(events).is_err() {
                recognition = None;
                let _sent = events.send(Event::Listening(false));
                let _sent = events.send(Event::Failed(if expired {
                    "Dictation stopped after two minutes. Review your text before sending."
                } else {
                    "Dictation stopped. Check your microphone and Windows speech settings."
                }));
            }
        }
        if let Some(until) = speech_until
            && (Instant::now() >= until || !voice.as_ref().is_some_and(speaking))
        {
            stop_speaking(voice.as_ref());
            speech_until = None;
            let _sent = events.send(Event::Speaking(false));
        }
    }
    drop(recognition);
    stop_speaking(voice.as_ref());
}

#[cfg(test)]
mod tests {
    use super::{Command, Event, MAX_TEXT, recognition_event, speech_text, start};
    use std::{io, time::Duration};
    use windows::Win32::Media::Speech::{SPEI_RECOGNITION, SPEVENT};

    #[test]
    fn speech_text_is_plain_bounded_and_nul_terminated() {
        assert_eq!(
            speech_text("<speak>hello</speak>"),
            Some("<speak>hello</speak>\0".encode_utf16().collect())
        );
        assert!(speech_text(" ").is_none());
        assert!(speech_text("hello\0hidden").is_none());
        assert!(speech_text(&"a".repeat(MAX_TEXT)).is_some());
        assert!(speech_text(&"a".repeat(MAX_TEXT + 1)).is_none());
        assert!(speech_text(&"🙂".repeat(MAX_TEXT / 2 + 1)).is_none());
    }

    #[test]
    fn only_owned_recognition_objects_are_interpreted_as_results() {
        let mut event = SPEVENT {
            _bitfield: SPEI_RECOGNITION.0 | (2 << 16),
            ..SPEVENT::default()
        };
        assert!(!recognition_event(&event));
        event.lParam.0 = 1;
        assert!(recognition_event(&event));
        event._bitfield = SPEI_RECOGNITION.0 | (4 << 16);
        assert!(!recognition_event(&event));
    }

    #[test]
    fn installed_speech_capabilities_do_not_capture_or_play_audio() -> io::Result<()> {
        let session = start()?;
        let event = session
            .events
            .recv_timeout(Duration::from_secs(20))
            .map_err(io::Error::other)?;
        assert!(matches!(event, Event::Capabilities { .. }));
        session
            .commands
            .send(Command::Shutdown)
            .map_err(io::Error::other)?;
        drop(session);
        Ok(())
    }
}
