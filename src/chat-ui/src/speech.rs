//! Cancellable Canary transcription and Qwen `VoiceDesign` synthesis workers.
//!
//! Permission and microphone capture stay on the native UI thread. Finished
//! clips and free-form voice descriptions cross this owned, text/audio boundary.
//! Generated WAV buffers live until native playback completes or is stopped.

#![allow(
    clippy::redundant_pub_crate,
    reason = "The window uses this crate-private worker boundary."
)]

use crate::{canary, native_speech::Service, tts};
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
// Short clips keep CPU synthesis within its bounded request deadline. The HTTP
// companion accepts larger inputs, but a readout retains and sequences all text.
const MAX_SEGMENT: usize = 256;
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
    Speaking {
        generation: u64,
        speaking: bool,
    },
    Failed {
        generation: u64,
        message: &'static str,
    },
    OutputStatus {
        generation: u64,
        message: &'static str,
    },
}

struct Envelope {
    command: Command,
    output_generation: u64,
}

/// Sending Stop/Speak invalidates synthesis immediately, even during HTTP I/O.
pub(crate) struct Commands {
    sender: Sender<Envelope>,
    current_output: Arc<AtomicU64>,
    current_input: Arc<AtomicU64>,
}
impl Commands {
    pub(crate) fn send(&self, command: Command) -> Result<(), mpsc::SendError<Command>> {
        if matches!(
            &command,
            Command::StopListening | Command::Speak { .. } | Command::Shutdown
        ) {
            self.current_input.store(NO_INPUT, Ordering::Release);
        }
        let generation = if matches!(
            &command,
            Command::Speak { .. }
                | Command::StopSpeaking
                | Command::Shutdown
                | Command::Transcribe { .. }
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
    #[cfg(test)]
    pub(crate) fn fixture() -> io::Result<(Self, Receiver<Command>)> {
        let (sender, incoming) = mpsc::channel::<Envelope>();
        let (outgoing, events) = mpsc::channel();
        let (observed, commands) = mpsc::channel();
        let current_input = Arc::new(AtomicU64::new(NO_INPUT));
        let worker = thread::Builder::new()
            .name("bonsai-speech-fixture".to_owned())
            .spawn(move || {
                let _events = outgoing;
                while let Ok(envelope) = incoming.recv() {
                    if matches!(envelope.command, Command::Shutdown) {
                        break;
                    }
                    if observed.send(envelope.command).is_err() {
                        break;
                    }
                }
            })?;
        Ok((
            Self {
                commands: Commands {
                    sender,
                    current_output: Arc::new(AtomicU64::new(0)),
                    current_input: Arc::clone(&current_input),
                },
                events,
                current_input,
                worker: Some(worker),
            },
            commands,
        ))
    }

    /// Queued output events from cancelled or replaced readouts cannot change UI state.
    pub(crate) fn is_current_output(&self, generation: u64) -> bool {
        self.commands.current_output.load(Ordering::Acquire) == generation
    }

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

/// Discover configured speech without opening audio devices or loading owned TTS.
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
            current_input: Arc::clone(&current_input),
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
        if let Some(worker) = self.worker.take() {
            stop_and_join(worker, stop_audio);
        }
    }
}

fn stop_and_join(worker: JoinHandle<()>, mut stop: impl FnMut()) {
    // A playback thread may have passed its generation check without entering
    // PlaySound yet. Repeating Stop until it exits covers that pre-start window:
    // a single Stop followed by join could miss the call and wait for the clip.
    // The worker owns the WAV allocation until synchronous playback returns.
    while !worker.is_finished() {
        stop();
        thread::sleep(Duration::from_millis(5));
    }
    let _joined = worker.join();
}

struct Output {
    generation: u64,
    voice: String,
    segments: VecDeque<String>,
    playback: Option<Playback>,
    prepared: bool,
    service: Option<Service>,
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

struct Input {
    client: canary::Client,
    config: canary::Config,
    generation: Option<u64>,
    service: Option<Service>,
}

impl Input {
    fn new(config: &canary::Config) -> Option<Self> {
        let client = canary::Client::new(config).ok()?;
        if config.launch.is_none() && client.ready().is_err() {
            return None;
        }
        Some(Self {
            client,
            config: config.clone(),
            generation: None,
            service: None,
        })
    }

    fn clear(&mut self) {
        self.service = None;
        self.generation = None;
    }

    fn transcribe(
        &mut self,
        generation: u64,
        wav: &[u8],
        current: &AtomicU64,
    ) -> Result<String, canary::Failure> {
        let call = canary::Call::new(generation, current, Duration::from_mins(15));
        call.check()?;
        if self.generation != Some(generation) {
            self.clear();
            self.service = self
                .client
                .start_service(&self.config, generation, wav, current)?;
            self.generation = Some(generation);
        }
        if let Some(service) = &self.service {
            service.check()?;
        }
        let result = self.client.transcribe(generation, wav, current);
        if let Some(service) = &self.service {
            service.check()?;
        }
        call.check()?;
        result
    }
}

fn transcription(
    input: Option<&mut Input>,
    generation: u64,
    wav: &[u8],
    current: &AtomicU64,
    events: &Sender<Event>,
) {
    if current.load(Ordering::Acquire) != generation {
        if let Some(input) = input {
            input.clear();
        }
        return;
    }
    let _sent = events.send(Event::Status("Transcribing with native Canary Qwen 2.5B. Microphone capture is paused; disabling it cancels model loading."));
    let result = input.map_or(
        Err(canary::Failure::Message(
            "Canary Qwen 2.5B is unavailable. Check the native speech configuration and key file.",
        )),
        |input| {
            let result = input.transcribe(generation, wav, current);
            if result.is_err() {
                input.clear();
            }
            result
        },
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
    config: Option<&tts::Config>,
    current: &Arc<AtomicU64>,
    events: &Sender<Event>,
) -> Result<bool, &'static str> {
    if current.load(Ordering::Acquire) != output.generation {
        return Ok(false);
    }
    if let Some(service) = &output.service {
        service.check().map_err(output_failure)?;
    }
    if !output.prepared {
        let client = client.ok_or("Qwen3-TTS VoiceDesign is unavailable. Check its installed runtime and protected key file.")?;
        let config = config.ok_or("Qwen3-TTS VoiceDesign has no local configuration.")?;
        let _sent = events.send(Event::OutputStatus {
            generation: output.generation,
            message: "Loading Qwen3-TTS VoiceDesign. Stop speech cancels loading.",
        });
        output.service = match client.start_service(config, output.generation, current) {
            Ok(service) => service,
            Err(tts::Failure::Cancelled) => return Ok(false),
            Err(tts::Failure::Message(message)) => return Err(message),
        };
        output.prepared = true;
        let _sent = events.send(Event::OutputStatus {
            generation: output.generation,
            message: "Generating speech with Qwen3-TTS VoiceDesign. Stop speech cancels synthesis.",
        });
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
    let synthesis = client.synthesize(&text, &output.voice, output.generation, current);
    // A pressure stop deliberately closes HTTP. Preserve its useful resource
    // reason instead of reducing it to a generic transport failure.
    if let Some(service) = &output.service {
        service.check().map_err(output_failure)?;
    }
    let wav = match synthesis {
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

const fn output_failure(failure: tts::Failure) -> &'static str {
    match failure {
        tts::Failure::Message(message) => message,
        tts::Failure::Cancelled => "Read aloud stopped.",
    }
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
    let mut input = canary_config.and_then(Input::new);
    let tts = tts_config
        .and_then(|config| tts::Client::new(config).ok())
        .filter(|client| {
            tts_config.is_some_and(|config| config.launch.is_some()) || client.ready().is_ok()
        });
    let _sent = events.send(Event::Capabilities {
        input: input.is_some(),
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
                output_generation,
            }) => {
                if current_input.load(Ordering::Acquire) != generation {
                    continue;
                }
                output = None;
                let _sent = events.send(Event::Speaking {
                    generation: output_generation,
                    speaking: false,
                });
                transcription(input.as_mut(), generation, &wav, current_input, events);
            }
            Ok(Envelope {
                command: Command::StopListening,
                ..
            }) => {
                if let Some(input) = &mut input {
                    input.clear();
                }
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
                if let Some(input) = &mut input {
                    input.clear();
                }
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
                        prepared: false,
                        service: None,
                    });
                    let _sent = events.send(Event::Speaking {
                        generation: output_generation,
                        speaking: true,
                    });
                } else {
                    let _sent = events.send(Event::Speaking {
                        generation: output_generation,
                        speaking: false,
                    });
                    let _sent = events.send(Event::Failed {
                        generation: output_generation,
                        message: "Read-aloud needs an answer up to 32,768 UTF-16 characters and a non-empty voice description up to 1,024 characters.",
                    });
                }
            }
            Ok(Envelope {
                command: Command::StopSpeaking,
                output_generation,
            }) => {
                output = None;
                let _sent = events.send(Event::Speaking {
                    generation: output_generation,
                    speaking: false,
                });
            }
            Err(RecvTimeoutError::Timeout) => {}
        }
        if let Some(input) = &mut input
            && input
                .generation
                .is_some_and(|generation| current_input.load(Ordering::Acquire) != generation)
        {
            input.clear();
        }
        if let Some(active) = &mut output {
            let generation = active.generation;
            let outcome = render(active, tts.as_ref(), tts_config, current_output, events);
            if !matches!(outcome, Ok(true)) {
                output = None;
                if current_output.load(Ordering::Acquire) == generation {
                    let _sent = events.send(Event::Speaking {
                        generation,
                        speaking: false,
                    });
                    if let Err(message) = outcome {
                        let _sent = events.send(Event::Failed {
                            generation,
                            message,
                        });
                    }
                }
            }
        }
    }
    drop(output);
}

#[cfg(test)]
mod tests {
    use super::{
        Command, Commands, Event, MAX_SEGMENT, MAX_TEXT, Session, segments, start, stop_and_join,
    };
    use std::{
        io,
        sync::{
            Arc,
            atomic::{AtomicBool, AtomicU64, Ordering},
            mpsc,
        },
        time::Duration,
    };

    #[test]
    fn switching_speech_modes_invalidates_the_other_generation_immediately() -> io::Result<()> {
        let (sender, _receiver) = mpsc::channel();
        let current_input = Arc::new(AtomicU64::new(7));
        let current_output = Arc::new(AtomicU64::new(3));
        let commands = Commands {
            sender,
            current_input: Arc::clone(&current_input),
            current_output: Arc::clone(&current_output),
        };
        commands
            .send(Command::Transcribe {
                generation: 7,
                wav: Vec::new(),
            })
            .map_err(io::Error::other)?;
        assert_eq!(current_input.load(Ordering::Acquire), 7);
        assert_ne!(current_output.load(Ordering::Acquire), 3);
        commands
            .send(Command::Speak {
                text: "Answer".to_owned(),
                voice_description: "Calm".to_owned(),
            })
            .map_err(io::Error::other)?;
        assert_eq!(current_input.load(Ordering::Acquire), super::NO_INPUT);
        current_input.store(8, Ordering::Release);
        commands
            .send(Command::StopListening)
            .map_err(io::Error::other)?;
        assert_eq!(current_input.load(Ordering::Acquire), super::NO_INPUT);
        Ok(())
    }

    #[test]
    fn stale_input_generation_releases_its_native_job_without_audio_or_transcript() -> io::Result<()>
    {
        use bonsai_windows_process::{OwnedProcess, ProcessSpec};
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(io::Error::other)?
            .as_nanos();
        let directory = std::env::temp_dir().join(format!("Bonsai-stale-input-{stamp}"));
        std::fs::create_dir(&directory)?;
        let credentials = bonsai_windows_credentials::PrivateCredentials::create(&directory)?;
        let config = crate::canary::Config {
            port: 8766,
            key_file: credentials.frontend_path().to_owned(),
            launch: None,
        };
        let process = Arc::new(OwnedProcess::start(&ProcessSpec {
            executable: std::env::current_exe()?,
            arguments: [
                "--exact",
                "native_speech::tests::resource_guard_fake_service",
                "--ignored",
            ]
            .map(std::ffi::OsString::from)
            .to_vec(),
            working_directory: directory.clone(),
            stdout: directory.join("fake.stdout"),
            stderr: directory.join("fake.stderr"),
            memory_limit_bytes: 512 * 1024 * 1024,
            active_process_limit: 1,
        })?);
        let service = crate::native_speech::Service::monitored(Arc::clone(&process), || {
            Ok(9 * 1024 * 1024 * 1024)
        })
        .map_err(|_| io::Error::other("Could not monitor disposable native service."))?;
        let mut input = super::Input {
            client: crate::canary::Client::new(&config).map_err(io::Error::other)?,
            config,
            generation: Some(41),
            service: Some(service),
        };
        let (events, received) = mpsc::channel();
        super::transcription(Some(&mut input), 41, &[], &AtomicU64::new(42), &events);
        assert!(input.service.is_none());
        assert!(input.generation.is_none());
        assert!(received.try_recv().is_err());
        assert!(process.wait_for_exit(Duration::from_secs(5))?.is_some());
        Ok(())
    }

    #[test]
    fn stop_retries_when_playback_starts_after_the_first_stop() -> io::Result<()> {
        let (start_sender, start_receiver) = mpsc::channel();
        let (stop_sender, stop_receiver) = mpsc::channel();
        let playing = Arc::new(AtomicBool::new(false));
        let stopped = Arc::new(AtomicBool::new(false));
        let worker_playing = Arc::clone(&playing);
        let worker_stopped = Arc::clone(&stopped);
        let worker = std::thread::Builder::new()
            .name("fake-late-playback".to_owned())
            .spawn(move || {
                // Deterministically reproduce the gap between the generation
                // check and entering a synchronous native playback operation.
                if start_receiver.recv_timeout(Duration::from_secs(2)).is_err() {
                    return;
                }
                worker_playing.store(true, Ordering::Release);
                if stop_receiver.recv_timeout(Duration::from_secs(2)).is_ok() {
                    worker_stopped.store(true, Ordering::Release);
                }
            })?;
        let mut first_stop = Some(start_sender);
        let mut stop_count = 0;
        stop_and_join(worker, || {
            stop_count += 1;
            if playing.load(Ordering::Acquire) {
                let _sent = stop_sender.send(());
            }
            if let Some(start) = first_stop.take() {
                let _sent = start.send(());
            }
        });
        assert!(stopped.load(Ordering::Acquire));
        assert!(stop_count >= 2);
        Ok(())
    }

    #[test]
    fn stopped_and_replaced_readouts_reject_queued_output_events() -> io::Result<()> {
        let (sender, receiver) = mpsc::channel();
        let (_events, events) = mpsc::channel();
        let session = Session {
            commands: Commands {
                sender,
                current_output: Arc::new(AtomicU64::new(0)),
                current_input: Arc::new(AtomicU64::new(super::NO_INPUT)),
            },
            events,
            current_input: Arc::new(AtomicU64::new(super::NO_INPUT)),
            worker: None,
        };
        session
            .commands
            .send(Command::Speak {
                text: "First answer".to_owned(),
                voice_description: "Warm".to_owned(),
            })
            .map_err(io::Error::other)?;
        let first = receiver.recv().map_err(io::Error::other)?.output_generation;
        assert!(session.is_current_output(first));
        session
            .commands
            .send(Command::StopSpeaking)
            .map_err(io::Error::other)?;
        let stopped = receiver.recv().map_err(io::Error::other)?.output_generation;
        assert!(!session.is_current_output(first));
        assert!(session.is_current_output(stopped));
        session
            .commands
            .send(Command::Speak {
                text: "Second answer".to_owned(),
                voice_description: "Calm".to_owned(),
            })
            .map_err(io::Error::other)?;
        let second = receiver.recv().map_err(io::Error::other)?.output_generation;
        assert!(!session.is_current_output(first));
        assert!(!session.is_current_output(stopped));
        assert!(session.is_current_output(second));
        Ok(())
    }

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
            current_input: Arc::new(AtomicU64::new(super::NO_INPUT)),
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

    #[test]
    fn owned_speech_discovery_does_not_launch_or_load_models_before_explicit_use() -> io::Result<()>
    {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(io::Error::other)?
            .as_nanos();
        let parent = std::env::temp_dir().join(format!("Bonsai-lazy-tts-{stamp}"));
        std::fs::create_dir(&parent)?;
        let evidence = parent.join("evidence");
        std::fs::create_dir(&evidence)?;
        let credentials = bonsai_windows_credentials::PrivateCredentials::create(&parent)?;
        let config = crate::tts::Config {
            port: 8767,
            key_file: credentials.frontend_path().to_owned(),
            launch: Some(crate::native_speech::Launch {
                executable: parent.join("unavailable-native.exe"),
                codec: Some(parent.join("unavailable-codec.gguf")),
                model: parent.join("unavailable-model.gguf"),
                evidence: evidence.clone(),
            }),
        };
        let canary_config = crate::canary::Config {
            port: 8766,
            key_file: credentials.frontend_path().to_owned(),
            launch: Some(crate::native_speech::Launch {
                executable: parent.join("unavailable-canary.exe"),
                model: parent.join("unavailable-canary.gguf"),
                codec: None,
                evidence: evidence.clone(),
            }),
        };
        let session = start(Some(canary_config.clone()), Some(config.clone()))?;
        assert!(matches!(
            session
                .events
                .recv_timeout(Duration::from_secs(2))
                .map_err(io::Error::other)?,
            Event::Capabilities {
                input: true,
                output: true
            }
        ));
        // Cancellation must also precede port observation, evidence creation,
        // process launch and dependency/model loading.
        let client = crate::tts::Client::new(&config).map_err(io::Error::other)?;
        assert!(matches!(
            client.start_service(&config, 1, &AtomicU64::new(2)),
            Err(crate::tts::Failure::Cancelled)
        ));
        let canary = crate::canary::Client::new(&canary_config).map_err(io::Error::other)?;
        assert!(matches!(
            canary.start_service(&canary_config, 1, &[], &AtomicU64::new(2)),
            Err(crate::canary::Failure::Cancelled)
        ));
        assert!(std::fs::read_dir(&evidence)?.next().is_none());
        drop(session);
        drop(client);
        drop(canary);
        drop(credentials);
        std::fs::remove_dir(&evidence)?;
        std::fs::remove_dir_all(parent)?;
        Ok(())
    }
}
