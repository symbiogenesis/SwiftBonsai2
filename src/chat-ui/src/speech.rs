//! Cancellable Canary transcription and Qwen `VoiceDesign` synthesis workers.
//!
//! Permission and microphone capture stay on the native UI thread. Finished
//! clips and free-form voice descriptions cross this owned, text/audio boundary.
//! Generated PCM stays in bounded queues until native playback completes or is stopped.

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
    time::{Duration, Instant},
};
mod command_queue;
mod pipeline;
mod segmentation;
mod stream;

use segmentation::segments;

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
    BeginStream {
        voice_description: String,
    },
    AppendStream {
        text: String,
    },
    FinishStream,
    StopSpeaking,
    OutputOverflow,
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
    sender: command_queue::Sender,
    current_output: Arc<AtomicU64>,
    current_input: Arc<AtomicU64>,
}
impl Commands {
    pub(crate) fn send(&self, command: Command) -> Result<(), mpsc::SendError<Command>> {
        if matches!(
            &command,
            Command::StopListening
                | Command::Speak { .. }
                | Command::BeginStream { .. }
                | Command::Shutdown
        ) {
            self.current_input.store(NO_INPUT, Ordering::Release);
        }
        let generation = if matches!(
            &command,
            Command::Speak { .. }
                | Command::BeginStream { .. }
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
            .send(
                Envelope {
                    command,
                    output_generation: generation,
                },
                &self.current_output,
            )
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
        let (sender, incoming) = command_queue::fixture_channel();
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
    let (sender, incoming) = command_queue::channel();
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

struct Output {
    generation: u64,
    voice: String,
    segments: VecDeque<String>,
    stream: Option<stream::Text>,
    failure: Option<&'static str>,
    pipeline: Option<pipeline::Pipeline>,
    prepared: bool,
    service: Option<Service>,
}

const WARM_IDLE: Duration = Duration::from_secs(30);

struct Idle<T> {
    value: T,
    until: Instant,
}

fn take_warm<T>(
    idle: &mut Option<Idle<T>>,
    now: Instant,
    healthy: impl FnOnce(&T) -> bool,
) -> Option<T> {
    idle.take()
        .filter(|warm| now < warm.until && healthy(&warm.value))
        .map(|warm| warm.value)
}

fn receive_command<T>(
    commands: &command_queue::Receiver,
    output: Option<&Output>,
    warm: Option<&Idle<T>>,
    input: Option<&Input>,
) -> Result<Envelope, RecvTimeoutError> {
    // Retained owners still need expiry, cancellation and health checks even
    // without incoming commands. A cold worker has no periodic work to do.
    if output.is_some()
        || warm.is_some()
        || input.is_some_and(|input| input.generation.is_some() || input.service.is_some())
    {
        commands.recv_timeout(POLL)
    } else {
        commands.recv()
    }
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
    client: Option<&Arc<tts::Client>>,
    config: Option<&tts::Config>,
    current: &Arc<AtomicU64>,
    events: &Sender<Event>,
) -> Result<bool, &'static str> {
    if current.load(Ordering::Acquire) != output.generation {
        return Ok(false);
    }
    if let Some(failure) = output.failure {
        return Err(failure);
    }
    if let Some(stream) = &mut output.stream {
        output.segments.extend(stream.ready(Instant::now()));
    }
    let finished = output.stream.as_ref().is_none_or(|stream| stream.finished);
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
    if output.pipeline.is_none() && output.segments.is_empty() {
        return Ok(!finished);
    }
    if output.pipeline.is_none() {
        let client = client.ok_or(
            "Qwen3-TTS VoiceDesign is unavailable. Check its companion service and key file.",
        )?;
        output.pipeline = Some(
            pipeline::Pipeline::start(
                Arc::clone(client),
                output.voice.clone(),
                output.generation,
                Arc::clone(current),
            )
            .map_err(|_| "Could not start native speech playback.")?,
        );
    }
    if let Some(pipeline) = &mut output.pipeline {
        pipeline.feed(&mut output.segments, finished);
    }
    match output
        .pipeline
        .as_ref()
        .and_then(pipeline::Pipeline::result)
    {
        None => Ok(true),
        Some(Ok(())) => Ok(false),
        Some(Err(failure)) => Err(output_failure(failure)),
    }
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
    commands: &command_queue::Receiver,
    events: &Sender<Event>,
    current_input: &AtomicU64,
    current_output: &Arc<AtomicU64>,
) {
    let mut input = canary_config.and_then(Input::new);
    let tts = tts_config
        .and_then(|config| tts::Client::new(config).ok())
        .map(Arc::new)
        .filter(|client| {
            tts_config.is_some_and(|config| config.launch.is_some()) || client.ready().is_ok()
        });
    let _sent = events.send(Event::Capabilities {
        input: input.is_some(),
        output: tts.is_some(),
    });
    let mut output: Option<Output> = None;
    let mut warm: Option<Idle<Service>> = None;
    loop {
        if warm.as_ref().is_some_and(|idle| {
            Instant::now() >= idle.until
                || idle.value.check().is_err()
                || current_input.load(Ordering::Acquire) != NO_INPUT
        }) {
            warm = None;
        }
        match receive_command(commands, output.as_ref(), warm.as_ref(), input.as_ref()) {
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
                warm = None;
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
                    warm = None;
                    continue;
                }
                if let Some(segments) = segments(&text)
                    && !voice_description.trim().is_empty()
                    && voice_description.chars().count() <= 1024
                    && !voice_description.contains('\0')
                {
                    let service =
                        take_warm(&mut warm, Instant::now(), |service| service.check().is_ok());
                    let prepared = service.is_some();
                    output = Some(Output {
                        generation: output_generation,
                        voice: voice_description,
                        segments,
                        stream: None,
                        failure: None,
                        pipeline: None,
                        prepared,
                        service,
                    });
                    let _sent = events.send(Event::Speaking {
                        generation: output_generation,
                        speaking: true,
                    });
                } else {
                    warm = None;
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
                command: Command::BeginStream { voice_description },
                output_generation,
            }) => {
                output = None;
                if let Some(input) = &mut input {
                    input.clear();
                }
                if current_output.load(Ordering::Acquire) != output_generation {
                    warm = None;
                    continue;
                }
                let service =
                    take_warm(&mut warm, Instant::now(), |service| service.check().is_ok());
                let failure = (voice_description.trim().is_empty()
                    || voice_description.chars().count() > 1024
                    || voice_description.contains('\0'))
                .then_some(
                    "Read aloud needs a non-empty voice description up to 1,024 characters.",
                );
                output = Some(Output {
                    generation: output_generation,
                    voice: voice_description,
                    segments: VecDeque::new(),
                    stream: Some(stream::Text::default()),
                    failure,
                    pipeline: None,
                    prepared: service.is_some(),
                    service,
                });
                let _sent = events.send(Event::Speaking {
                    generation: output_generation,
                    speaking: true,
                });
            }
            Ok(Envelope {
                command: Command::AppendStream { text },
                output_generation,
            }) => {
                if let Some(active) = &mut output
                    && active.generation == output_generation
                    && let Some(stream) = &mut active.stream
                    && let Err(message) = stream.append(&text, Instant::now())
                {
                    active.failure = Some(message);
                }
            }
            Ok(Envelope {
                command: Command::FinishStream,
                output_generation,
            }) => {
                if let Some(active) = &mut output
                    && active.generation == output_generation
                    && let Some(stream) = &mut active.stream
                {
                    stream.finished = true;
                }
            }
            Ok(Envelope {
                command: Command::OutputOverflow,
                output_generation,
            }) => {
                output = None;
                warm = None;
                let _sent = events.send(Event::Failed {
                    generation: output_generation,
                    message: command_queue::OVERFLOW_MESSAGE,
                });
            }
            Ok(Envelope {
                command: Command::StopSpeaking,
                output_generation,
            }) => {
                output = None;
                warm = None;
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
                if matches!(outcome, Ok(false))
                    && current_output.load(Ordering::Acquire) == generation
                {
                    warm = active
                        .service
                        .take()
                        .filter(|service| service.check().is_ok())
                        .map(|service| Idle {
                            value: service,
                            until: Instant::now() + WARM_IDLE,
                        });
                } else {
                    warm = None;
                }
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
    use super::{Command, Commands, Event, MAX_SEGMENT, MAX_TEXT, Session, segments, start};
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
    fn streaming_append_and_finish_preserve_generation_until_replaced() -> io::Result<()> {
        let (sender, receiver) = super::command_queue::channel();
        let current_output = Arc::new(AtomicU64::new(0));
        let current_input = Arc::new(AtomicU64::new(7));
        let commands = Commands {
            sender,
            current_output: Arc::clone(&current_output),
            current_input: Arc::clone(&current_input),
        };
        commands
            .send(Command::BeginStream {
                voice_description: "Calm".to_owned(),
            })
            .map_err(io::Error::other)?;
        let generation = receiver.recv().map_err(io::Error::other)?.output_generation;
        assert_eq!(current_input.load(Ordering::Acquire), super::NO_INPUT);
        for command in [
            Command::AppendStream {
                text: "First sentence. ".to_owned(),
            },
            Command::FinishStream,
        ] {
            commands.send(command).map_err(io::Error::other)?;
            assert_eq!(
                receiver.recv().map_err(io::Error::other)?.output_generation,
                generation
            );
            assert_eq!(current_output.load(Ordering::Acquire), generation);
        }
        commands
            .send(Command::Speak {
                text: "Replay".to_owned(),
                voice_description: "Warm".to_owned(),
            })
            .map_err(io::Error::other)?;
        assert_ne!(
            receiver.recv().map_err(io::Error::other)?.output_generation,
            generation
        );
        Ok(())
    }

    #[test]
    fn warm_service_reuse_requires_health_and_unexpired_idle_time() {
        use super::{Idle, WARM_IDLE, take_warm};
        use std::{sync::atomic::AtomicUsize, time::Instant};
        struct Probe(Arc<AtomicUsize>);
        impl Drop for Probe {
            fn drop(&mut self) {
                self.0.fetch_add(1, Ordering::AcqRel);
            }
        }
        let drops = Arc::new(AtomicUsize::new(0));
        let now = Instant::now();
        let mut idle = Some(Idle {
            value: Probe(Arc::clone(&drops)),
            until: now + WARM_IDLE,
        });
        let retained = take_warm(&mut idle, now, |_| true);
        assert!(retained.is_some() && idle.is_none());
        assert_eq!(drops.load(Ordering::Acquire), 0);
        drop(retained);
        for (at, healthy) in [(now + WARM_IDLE, true), (now, false)] {
            idle = Some(Idle {
                value: Probe(Arc::clone(&drops)),
                until: now + WARM_IDLE,
            });
            assert!(take_warm(&mut idle, at, |_| healthy).is_none());
            assert!(idle.is_none());
        }
        assert_eq!(drops.load(Ordering::Acquire), 3);
    }

    #[test]
    fn cold_speech_worker_waits_for_commands_and_wakes_for_stop_and_shutdown() -> io::Result<()> {
        let (sender, receiver) = super::command_queue::channel();
        let waiting = receiver.observe_waits()?;
        let current_input = Arc::new(AtomicU64::new(super::NO_INPUT));
        let current_output = Arc::new(AtomicU64::new(0));
        let commands = Commands {
            sender,
            current_input: Arc::clone(&current_input),
            current_output: Arc::clone(&current_output),
        };
        let (events, received) = mpsc::channel();
        let (complete, finished) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            super::run(
                None,
                None,
                &receiver,
                &events,
                &current_input,
                &current_output,
            );
            let _sent = complete.send(());
        });
        let guard = Duration::from_secs(2);
        assert!(matches!(
            received.recv_timeout(guard).map_err(io::Error::other)?,
            Event::Capabilities {
                input: false,
                output: false
            }
        ));
        assert_eq!(waiting.recv_timeout(guard).map_err(io::Error::other)?, None);
        commands
            .send(Command::StopSpeaking)
            .map_err(io::Error::other)?;
        let generation = commands.current_output.load(Ordering::Acquire);
        assert!(matches!(
            received.recv_timeout(guard).map_err(io::Error::other)?,
            Event::Speaking { generation: stopped, speaking: false } if stopped == generation
        ));
        assert_eq!(waiting.recv_timeout(guard).map_err(io::Error::other)?, None);
        commands.send(Command::Shutdown).map_err(io::Error::other)?;
        finished.recv_timeout(guard).map_err(io::Error::other)?;
        worker
            .join()
            .map_err(|_| io::Error::other("Speech fixture worker failed"))?;
        Ok(())
    }

    #[test]
    fn speech_command_wait_preserves_monitoring_for_warm_active_and_retained_input()
    -> io::Result<()> {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(io::Error::other)?
            .as_nanos();
        let directory = std::env::temp_dir().join(format!("Bonsai-speech-waits-{stamp}"));
        std::fs::create_dir(&directory)?;
        let credentials = bonsai_windows_credentials::PrivateCredentials::create(&directory)?;
        let config = crate::canary::Config {
            port: 8766,
            key_file: credentials.frontend_path().to_owned(),
            launch: None,
        };
        for (active, warm, generation) in [
            (false, false, None),
            (true, false, None),
            (false, true, None),
            (false, false, Some(41)),
        ] {
            let input = super::Input {
                client: crate::canary::Client::new(&config).map_err(io::Error::other)?,
                config: config.clone(),
                generation,
                service: None,
            };
            let output = active.then(|| super::Output {
                generation: 0,
                voice: "Calm".to_owned(),
                segments: super::VecDeque::new(),
                stream: Some(super::stream::Text::default()),
                failure: None,
                pipeline: None,
                prepared: false,
                service: None,
            });
            let warm = warm.then(|| super::Idle {
                value: (),
                until: std::time::Instant::now() + super::WARM_IDLE,
            });
            let monitored = active || warm.is_some() || generation.is_some();
            let (sender, receiver) = super::command_queue::channel();
            let waiting = receiver.observe_waits()?;
            let (complete, finished) = mpsc::channel();
            let worker = std::thread::spawn(move || {
                loop {
                    match super::receive_command(
                        &receiver,
                        output.as_ref(),
                        warm.as_ref(),
                        Some(&input),
                    ) {
                        Err(mpsc::RecvTimeoutError::Timeout) => {}
                        command => {
                            let _sent = complete.send(command);
                            break;
                        }
                    }
                }
            });
            let guard = Duration::from_secs(2);
            assert_eq!(
                waiting.recv_timeout(guard).map_err(io::Error::other)?,
                monitored.then_some(super::POLL)
            );
            sender
                .send(
                    super::Envelope {
                        command: Command::StopSpeaking,
                        output_generation: 1,
                    },
                    &AtomicU64::new(1),
                )
                .map_err(|_| io::Error::other("Could not wake speech fixture"))?;
            assert!(matches!(
                finished.recv_timeout(guard).map_err(io::Error::other)?,
                Ok(super::Envelope {
                    command: Command::StopSpeaking,
                    output_generation: 1
                })
            ));
            worker
                .join()
                .map_err(|_| io::Error::other("Speech wait fixture failed"))?;
        }
        drop(credentials);
        std::fs::remove_dir_all(directory)?;
        Ok(())
    }

    #[test]
    fn switching_speech_modes_invalidates_the_other_generation_immediately() -> io::Result<()> {
        let (sender, _receiver) = super::command_queue::channel();
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
    fn stopped_and_replaced_readouts_reject_queued_output_events() -> io::Result<()> {
        let (sender, receiver) = super::command_queue::channel();
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
        let (sender, receiver) = super::command_queue::channel();
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
                voice_model: Some(parent.join("unavailable-voice.gguf")),
                quantization: None,
                threads: None,
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
                voice_model: None,
                quantization: None,
                threads: None,
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
