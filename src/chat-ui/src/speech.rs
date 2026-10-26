//! Cancellable Canary transcription and Qwen `VoiceDesign` synthesis workers.
//!
//! Permission and microphone capture stay on the native UI thread. Finished
//! clips and free-form voice descriptions cross this owned, text/audio boundary.
//! Generated WAV buffers live until native playback completes or is stopped.

#![allow(
    clippy::redundant_pub_crate,
    reason = "The window uses this crate-private worker boundary."
)]

use crate::{canary, tts};
use std::{
    collections::VecDeque,
    io,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
        mpsc::{self, Receiver, RecvTimeoutError, Sender},
    },
    thread::{self, JoinHandle},
    time::Duration,
};
use windows_sys::Win32::Media::Audio::{PlaySoundW, SND_MEMORY, SND_NODEFAULT, SND_SYNC};

const POLL: Duration = Duration::from_millis(50);
const MAX_TEXT: usize = 32768;
const MAX_SEGMENT: usize = 2048;
const NO_INPUT: u64 = u64::MAX;

pub(crate) enum Command {
    Transcribe {
        generation: u64,
        wav: Vec<u8>,
    },
    StopListening,
    Speak {
        text: String,
        voice_description: String,
    },
    StopSpeaking,
    Shutdown,
}

pub(crate) enum Event {
    Capabilities {
        input: bool,
        output: bool,
    },
    Listening(bool),
    Recognized {
        generation: u64,
        text: String,
    },
    InputFailed {
        generation: u64,
        message: &'static str,
    },
    Status(&'static str),
    Speaking(bool),
    Failed(&'static str),
}

struct Envelope {
    command: Command,
    output_generation: u64,
}

/// Sending Stop/Speak invalidates synthesis immediately, even during HTTP I/O.
pub(crate) struct Commands {
    sender: Sender<Envelope>,
    current_output: Arc<AtomicU64>,
}
impl Commands {
    pub(crate) fn send(&self, command: Command) -> Result<(), mpsc::SendError<Command>> {
        let generation = if matches!(
            &command,
            Command::Speak { .. } | Command::StopSpeaking | Command::Shutdown
        ) {
            self.current_output
                .fetch_add(1, Ordering::AcqRel)
                .wrapping_add(1)
        } else {
            self.current_output.load(Ordering::Acquire)
        };
        self.sender
            .send(Envelope {
                command,
                output_generation: generation,
            })
            .map_err(|error| mpsc::SendError(error.0.command))
    }
}

pub(crate) struct Session {
    pub(crate) commands: Commands,
    pub(crate) events: Receiver<Event>,
    current_input: Arc<AtomicU64>,
    worker: Option<JoinHandle<()>>,
}
impl Session {
    /// The UI calls this before disabling capture, sending text, or closing.
    pub(crate) fn set_input_generation(&self, generation: Option<u64>) {
        self.current_input
            .store(generation.unwrap_or(NO_INPUT), Ordering::Release);
    }
}
impl Drop for Session {
    fn drop(&mut self) {
        self.set_input_generation(None);
        let _sent = self.commands.send(Command::Shutdown);
        if let Some(worker) = self.worker.take() {
            let _joined = worker.join();
        }
    }
}

/// Discover both explicitly configured services without opening audio devices.
pub(crate) fn start(
    canary_config: Option<canary::Config>,
    tts_config: Option<tts::Config>,
) -> io::Result<Session> {
    let (sender, incoming) = mpsc::channel();
    let (outgoing, events) = mpsc::channel();
    let current_input = Arc::new(AtomicU64::new(NO_INPUT));
    let current_output = Arc::new(AtomicU64::new(0));
    let input = Arc::clone(&current_input);
    let output = Arc::clone(&current_output);
    let worker = thread::Builder::new()
        .name("bonsai-speech".to_owned())
        .spawn(move || {
            run(
                canary_config.as_ref(),
                tts_config.as_ref(),
                &incoming,
                &outgoing,
                &input,
                &output,
            );
        })?;
    Ok(Session {
        commands: Commands {
            sender,
            current_output,
        },
        events,
        current_input,
        worker: Some(worker),
    })
}

fn stop_audio() {
    // SAFETY: A null sound stops this process's PlaySound playback. No input
    // pointers or handles are retained. Only this module starts native audio.
    unsafe { PlaySoundW(std::ptr::null(), std::ptr::null_mut(), 0) };
}

struct Playback {
    finished: Receiver<bool>,
    worker: Option<JoinHandle<()>>,
}
impl Playback {
    fn start(wav: Vec<u8>, generation: u64, current: Arc<AtomicU64>) -> io::Result<Self> {
        let (events, finished) = mpsc::channel();
        let worker = thread::Builder::new()
            .name("bonsai-qwen-audio".to_owned())
            .spawn(move || {
                let played = if current.load(Ordering::Acquire) == generation {
                    // SAFETY: The TTS adapter validated this complete bounded PCM WAV.
                    // SND_MEMORY interprets the pointer as audio bytes, not a path.
                    // SND_SYNC keeps the owned allocation alive until playback stops
                    // or finishes; SND_NODEFAULT forbids substituted system sounds.
                    (unsafe {
                        PlaySoundW(
                            wav.as_ptr().cast(),
                            std::ptr::null_mut(),
                            SND_MEMORY | SND_SYNC | SND_NODEFAULT,
                        )
                    }) != 0
                } else {
                    false
                };
                let _sent = events.send(played);
            })?;
        Ok(Self {
            finished,
            worker: Some(worker),
        })
    }
    fn result(&self) -> Option<bool> {
        match self.finished.try_recv() {
            Ok(result) => Some(result),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => Some(false),
        }
    }
}
impl Drop for Playback {
    fn drop(&mut self) {
        stop_audio();
        if let Some(worker) = self.worker.take() {
            let _joined = worker.join();
        }
    }
}

struct Output {
    generation: u64,
    voice: String,
    segments: VecDeque<String>,
    playback: Option<Playback>,
}

fn segments(text: &str) -> Option<VecDeque<String>> {
    if text.trim().is_empty()
        || text.contains('\0')
        || text.encode_utf16().take(MAX_TEXT + 1).count() > MAX_TEXT
    {
        return None;
    }
    let mut remaining = text.trim();
    let mut result = VecDeque::new();
    while !remaining.is_empty() {
        let maximum = remaining
            .char_indices()
            .nth(MAX_SEGMENT)
            .map_or(remaining.len(), |(index, _)| index);
        let boundary = if maximum < remaining.len() {
            remaining[..maximum]
                .char_indices()
                .rev()
                .find(|(index, character)| *index >= maximum / 2 && character.is_whitespace())
                .map_or(maximum, |(index, _)| index)
        } else {
            maximum
        };
        result.push_back(remaining[..boundary].trim().to_owned());
        remaining = remaining[boundary..].trim_start();
    }
    Some(result)
}

fn transcription(
    client: Option<&canary::Client>,
    generation: u64,
    wav: &[u8],
    current: &AtomicU64,
    events: &Sender<Event>,
) {
    if current.load(Ordering::Acquire) != generation {
        return;
    }
    let _sent = events.send(Event::Status(
        "Transcribing with Canary Qwen 2.5B. Microphone capture is paused.",
    ));
    let result = client.map_or(
        Err(canary::Failure::Message(
            "Canary Qwen 2.5B is unavailable. Check its companion service and key file.",
        )),
        |client| client.transcribe(generation, wav, current),
    );
    if current.load(Ordering::Acquire) != generation {
        return;
    }
    match result {
        Ok(text) => {
            let _sent = events.send(Event::Recognized { generation, text });
        }
        Err(canary::Failure::Cancelled) => {}
        Err(canary::Failure::Message(message)) => {
            let _sent = events.send(Event::InputFailed {
                generation,
                message,
            });
        }
    }
}

fn render(
    output: &mut Output,
    client: Option<&tts::Client>,
    current: &Arc<AtomicU64>,
) -> Result<bool, &'static str> {
    if current.load(Ordering::Acquire) != output.generation {
        return Ok(false);
    }
    if let Some(playback) = &output.playback {
        match playback.result() {
            None => return Ok(true),
            Some(true) => output.playback = None,
            Some(false) => {
                return Err("Audio playback failed. Check the Windows audio output device.");
            }
        }
    }
    let Some(text) = output.segments.pop_front() else {
        return Ok(false);
    };
    let client = client
        .ok_or("Qwen3-TTS VoiceDesign is unavailable. Check its companion service and key file.")?;
    let wav = match client.synthesize(&text, &output.voice, output.generation, current) {
        Ok(wav) => wav,
        Err(tts::Failure::Cancelled) => return Ok(false),
        Err(tts::Failure::Message(message)) => return Err(message),
    };
    if current.load(Ordering::Acquire) != output.generation {
        return Ok(false);
    }
    output.playback = Some(
        Playback::start(wav, output.generation, Arc::clone(current))
            .map_err(|_| "Could not start native audio playback.")?,
    );
    Ok(true)
}

#[allow(
    clippy::too_many_lines,
    reason = "One bounded worker owns command ordering, cancellation and the audio buffer lifetime."
)]
fn run(
    canary_config: Option<&canary::Config>,
    tts_config: Option<&tts::Config>,
    commands: &Receiver<Envelope>,
    events: &Sender<Event>,
    current_input: &AtomicU64,
    current_output: &Arc<AtomicU64>,
) {
    let canary = canary_config
        .and_then(|config| canary::Client::new(config).ok())
        .filter(|client| client.ready().is_ok());
    let tts = tts_config
        .and_then(|config| tts::Client::new(config).ok())
        .filter(|client| client.ready().is_ok());
    let _sent = events.send(Event::Capabilities {
        input: canary.is_some(),
        output: tts.is_some(),
    });
    let mut output: Option<Output> = None;
    loop {
        match commands.recv_timeout(POLL) {
            Ok(Envelope {
                command: Command::Shutdown,
                ..
            })
            | Err(RecvTimeoutError::Disconnected) => break,
            Ok(Envelope {
                command: Command::Transcribe { generation, wav },
                ..
            }) => {
                if current_input.load(Ordering::Acquire) != generation {
                    continue;
                }
                output = None;
                let _sent = events.send(Event::Speaking(false));
                transcription(canary.as_ref(), generation, &wav, current_input, events);
            }
            Ok(Envelope {
                command: Command::StopListening,
                ..
            }) => {
                let _sent = events.send(Event::Listening(false));
            }
            Ok(Envelope {
                command:
                    Command::Speak {
                        text,
                        voice_description,
                    },
                output_generation,
            }) => {
                output = None;
                if current_output.load(Ordering::Acquire) != output_generation {
                    continue;
                }
                if let Some(segments) = segments(&text)
                    && !voice_description.trim().is_empty()
                    && voice_description.chars().count() <= 1024
                    && !voice_description.contains('\0')
                {
                    output = Some(Output {
                        generation: output_generation,
                        voice: voice_description,
                        segments,
                        playback: None,
                    });
                    let _sent = events.send(Event::Speaking(true));
                } else {
                    let _sent = events.send(Event::Speaking(false));
                    let _sent = events.send(Event::Failed("Read-aloud needs an answer up to 32,768 UTF-16 characters and a non-empty voice description up to 1,024 characters."));
                }
            }
            Ok(Envelope {
                command: Command::StopSpeaking,
                ..
            }) => {
                output = None;
                let _sent = events.send(Event::Speaking(false));
            }
            Err(RecvTimeoutError::Timeout) => {}
        }
        if let Some(active) = &mut output {
            let generation = active.generation;
            let outcome = render(active, tts.as_ref(), current_output);
            if !matches!(outcome, Ok(true)) {
                output = None;
                if current_output.load(Ordering::Acquire) == generation {
                    let _sent = events.send(Event::Speaking(false));
                    if let Err(message) = outcome {
                        let _sent = events.send(Event::Failed(message));
                    }
                }
            }
        }
    }
    drop(output);
}

#[cfg(test)]
mod tests {
    use super::{Command, Commands, Event, MAX_SEGMENT, MAX_TEXT, segments, start};
    use std::{
        io,
        sync::{
            Arc,
            atomic::{AtomicU64, Ordering},
            mpsc,
        },
        time::Duration,
    };

    #[test]
    fn output_segments_preserve_unicode_and_bound_each_model_request() -> io::Result<()> {
        let text = "A calm sentence.  🙂 Another sentence. ".repeat(300);
        let pieces =
            segments(&text).ok_or_else(|| io::Error::other("Expected speech segments."))?;
        assert!(pieces.len() > 1);
        assert!(
            pieces
                .iter()
                .all(|part| part.chars().count() <= MAX_SEGMENT)
        );
        let original: String = text
            .chars()
            .filter(|character| !character.is_whitespace())
            .collect();
        let rebuilt: String = pieces
            .iter()
            .flat_map(|part| part.chars())
            .filter(|character| !character.is_whitespace())
            .collect();
        assert_eq!(original, rebuilt);
        assert!(segments(" ").is_none());
        assert!(segments("hello\0hidden").is_none());
        assert!(segments(&"a".repeat(MAX_TEXT + 1)).is_none());
        Ok(())
    }

    #[test]
    fn stopping_output_invalidates_queued_synthesis_before_worker_receives_it() -> io::Result<()> {
        let (sender, receiver) = mpsc::channel();
        let current_output = Arc::new(AtomicU64::new(0));
        let commands = Commands {
            sender,
            current_output: Arc::clone(&current_output),
        };
        commands
            .send(Command::Speak {
                text: "Hello".to_owned(),
                voice_description: "Warm and calm".to_owned(),
            })
            .map_err(io::Error::other)?;
        let queued = receiver.recv().map_err(io::Error::other)?;
        assert_eq!(
            queued.output_generation,
            current_output.load(Ordering::Acquire)
        );
        commands
            .send(Command::StopSpeaking)
            .map_err(io::Error::other)?;
        assert_ne!(
            queued.output_generation,
            current_output.load(Ordering::Acquire)
        );
        Ok(())
    }

    #[test]
    fn absent_services_do_not_open_audio_devices_or_substitute_system_voices() -> io::Result<()> {
        let session = start(None, None)?;
        let event = session
            .events
            .recv_timeout(Duration::from_secs(2))
            .map_err(io::Error::other)?;
        assert!(matches!(
            event,
            Event::Capabilities {
                input: false,
                output: false
            }
        ));
        session
            .commands
            .send(Command::Shutdown)
            .map_err(io::Error::other)?;
        drop(session);
        Ok(())
    }
}
