//! Owned, bounded loopback client for the native chat window.

#![forbid(unsafe_code)]
#![allow(
    clippy::redundant_pub_crate,
    reason = "The window and worker share a deliberately crate-private command boundary"
)]

use serde::{Deserialize, Serialize};
#[cfg(windows)]
use std::os::windows::process::CommandExt;
use std::{
    fs::{File, OpenOptions},
    io::{self, BufReader, Read, Write},
    net::{Ipv4Addr, SocketAddr, SocketAddrV4, TcpStream},
    path::{Path, PathBuf},
    process::{Child, Command as ProcessCommand, Stdio},
    sync::mpsc::{Receiver, RecvTimeoutError, TryRecvError},
    thread,
    time::{Duration, Instant},
};

#[cfg(test)]
mod cancellation_tests;
mod effort;
mod event_queue;

pub(crate) use event_queue::{EventReceiver, EventSender, channel as event_channel};

const POLL: Duration = Duration::from_millis(100);
const IO_POLL: Duration = Duration::from_millis(50);
const DEADLINE: Duration = Duration::from_secs(600);
const MAX_HEAD: usize = 16 * 1024;
const MAX_BODY: usize = 4 * 1024 * 1024;
const MAX_REQUEST: usize = 1024 * 1024;
const MAX_EVENT: usize = 1024 * 1024;
const MAX_ERROR_BODY: usize = 256;
const READ_SIZE: usize = 8192;
const EVIDENCE_PREFIX: &str = "Protected credentials and technical evidence: ";
const RUNNING_MEMORY_MESSAGE: &str = "Swift stopped because its running memory requirements were no longer met. Close other apps, then select Retry model. Completed conversation turns remain in memory.";
const LOCAL_CONNECTION_CLOSED: &str = "The connection to the model on this computer closed before a complete reply was received. The cause was not reported.";
const CONTEXT_LIMIT_MESSAGE: &str = "This conversation and the selected output cap do not fit within the model's 8,192-token context limit. Choose a smaller output cap or start a new conversation, then send again.";
pub(crate) const STOPPED_MESSAGE: &str = "Response stopped. Select Retry model to continue. Completed conversation turns are retained; the interrupted turn is not added to model history.";

pub(crate) const DEFAULT_ABLITERATION_SCALE: f32 = 1.3;

pub(crate) const fn default_abliteration_scale() -> f32 {
    DEFAULT_ABLITERATION_SCALE
}

pub(crate) fn valid_abliteration_scale(scale: f32) -> bool {
    scale.is_finite() && (0.0..=2.0).contains(&scale)
}

/// Fixed development launch inputs; paths and keys have no debug representation.
#[derive(Clone)]
pub(crate) struct Config {
    pub(crate) broker: PathBuf,
    pub(crate) data: PathBuf,
    pub(crate) evidence: PathBuf,
    pub(crate) frontend_port: u16,
    pub(crate) backend_port: u16,
    pub(crate) use_abliterate_lora: bool,
    pub(crate) use_repacked_ptq1: bool,
    pub(crate) canary: Option<crate::canary::Config>,
    pub(crate) qwen_tts: Option<crate::tts::Config>,
}

/// Commands sent from the native UI to its single background worker.
pub(crate) enum Command {
    Send {
        text: String,
        max_completion_tokens: u32,
        extra_effort: bool,
        enable_thinking: bool,
        abliteration_scale: f32,
    },
    NewChat,
    RestoreChat(Vec<Message>),
    Stop,
    Retry,
    Shutdown,
}

/// Text-only notifications; no credentials or raw HTTP diagnostics cross this boundary.
#[derive(Clone)]
pub(crate) enum Event {
    Starting,
    Ready,
    Text(String),
    Reasoning(String),
    Complete(Completion),
    Committed(Vec<Message>),
    Failed(String),
    Retryable(String),
    Stopping(String),
    Stopped,
    Rejected {
        message: String,
        text: String,
        retry_required: bool,
    },
    Stage(&'static str),
    Draft(String),
    // Extra effort restores a previously completed draft as one lifecycle
    // operation. Its aggregate body retains the separate MAX_BODY bound.
    RetainedAnswer(String),
}

/// Why a successfully framed generation ended; a token cap can leave partial prose.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Completion {
    Finished,
    TokenLimit,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Failure {
    Shutdown,
    Stopped,
    Memory,
    Rejected(&'static str),
    Message(&'static str),
}

type Result<T> = std::result::Result<T, Failure>;

struct Broker(Child);

impl Drop for Broker {
    fn drop(&mut self) {
        // The broker's kill-on-close Job owns the model process. Killing the
        // broker closes that Job even when the window exits during generation.
        let _kill = self.0.kill();
        let _wait = self.0.wait();
    }
}

#[derive(Clone, Serialize)]
pub(crate) struct Message {
    pub(crate) role: &'static str,
    pub(crate) content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) reasoning_content: Option<String>,
}

impl<'de> Deserialize<'de> for Message {
    fn deserialize<D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct StoredMessage {
            role: String,
            content: String,
            reasoning_content: Option<String>,
        }
        let stored = StoredMessage::deserialize(deserializer)?;
        let role = match stored.role.as_str() {
            "user" => "user",
            "assistant" => "assistant",
            _ => return Err(serde::de::Error::custom("Unsupported conversation role.")),
        };
        Ok(Self {
            role,
            content: stored.content,
            reasoning_content: stored.reasoning_content,
        })
    }
}

#[derive(Serialize)]
struct ChatRequest<'a> {
    messages: &'a [Message],
    max_completion_tokens: u32,
    stream: bool,
    enable_thinking: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    abliteration_scale: Option<f32>,
}

#[derive(Clone, Copy)]
struct ResponseOptions {
    max_completion_tokens: u32,
    enable_thinking: bool,
    abliteration_scale: Option<f32>,
}

/// Launch the owned broker, then serve one in-memory conversation until shutdown.
#[allow(
    clippy::needless_pass_by_value,
    reason = "The background thread owns its configuration and channels until the broker is stopped"
)]
pub(crate) fn run(config: Config, commands: Receiver<Command>, events: EventSender) {
    // Only committed turns live here, outside an individual owned broker's
    // lifetime. Explicit retries preserve them without replaying a failed turn.
    let mut history = Vec::new();
    for attempt in 0..u32::MAX {
        let attempt_config = Config {
            evidence: config.evidence.join(format!("attempt-{attempt}")),
            ..config.clone()
        };
        if std::fs::create_dir(&attempt_config.evidence).is_err() {
            let _notification = events.send(Event::Failed(
                "Could not create a fresh model startup directory. Restart the app.".to_owned(),
            ));
            return;
        }
        if notify(&events, Event::Starting).is_err() {
            return;
        }
        match worker(&attempt_config, &commands, &events, &mut history) {
            Ok(()) | Err(Failure::Shutdown) => return,
            Err(Failure::Memory) => {
                if !wait_for_retry(
                    &commands,
                    &events,
                    &mut history,
                    &Event::Retryable("Not enough free memory. Swift needs 16 GiB available. Free some RAM, then select Retry model.".to_owned()),
                ) {
                    return;
                }
            }
            Err(Failure::Stopped) => {
                // The broker guard has finished teardown. A requested stop is
                // an expected outcome, separate from failure/rejection notices.
                if !wait_for_retry(&commands, &events, &mut history, &Event::Stopped) {
                    return;
                }
            }
            Err(failure) => {
                let message = match failure {
                    Failure::Message(message) | Failure::Rejected(message) => message,
                    Failure::Shutdown | Failure::Memory | Failure::Stopped => return,
                };
                // worker has returned and its Broker guard has stopped/waited
                // for the owned process before Retry can be offered.
                if !wait_for_retry(&commands, &events, &mut history, &Event::Retryable(retry_guidance(message))) {
                    return;
                }
            }
        }
    }
}

fn retry_guidance(message: &str) -> String {
    if message == RUNNING_MEMORY_MESSAGE {
        return message.to_owned();
    }
    format!(
        "{message}\nSelect Retry model to restart the local model. Completed turns remain in memory."
    )
}

fn wait_for_retry(
    commands: &Receiver<Command>,
    events: &EventSender,
    history: &mut Vec<Message>,
    notice: &Event,
) -> bool {
    if notify(events, notice.clone()).is_err() {
        return false;
    }
    loop {
        match commands.recv_timeout(POLL) {
            Ok(Command::Retry) => return true,
            Ok(Command::Shutdown) | Err(RecvTimeoutError::Disconnected) => {
                return false;
            }
            Ok(Command::Send { text, .. }) => {
                // An idle broker can exit just as the UI submits. Restore that
                // queued prompt instead of discarding it, and retain the retry
                // state without automatically replaying the request.
                if notify(events, Event::Rejected {
                    message: "The model is unavailable. Your message is restored; select Retry model before sending again.".to_owned(),
                    text,
                    retry_required: true,
                }).is_err() || notify(events, notice.clone()).is_err() {
                    return false;
                }
            }
            // The UI clears the visible conversation immediately. Honor a
            // reset queued just before idle process death so a fresh transcript
            // cannot silently retain the previous conversation after Retry.
            Ok(Command::NewChat) => history.clear(),
            Ok(Command::RestoreChat(restored)) => *history = restored,
            Ok(Command::Stop) | Err(RecvTimeoutError::Timeout) => {}
        }
    }
}

fn worker(
    config: &Config,
    commands: &Receiver<Command>,
    events: &EventSender,
    history: &mut Vec<Message>,
) -> Result<()> {
    let stdout_path = config.evidence.join("broker.stdout.log");
    let stderr_path = config.evidence.join("broker.stderr.log");
    let stdout = new_log(&stdout_path)?;
    let stderr = new_log(&stderr_path)?;
    let mut process = ProcessCommand::new(&config.broker);
    if config.use_abliterate_lora {
        process.arg("--use-abliterate-lora");
    }
    if config.use_repacked_ptq1 {
        process.arg("--use-repacked-ptq1");
    }
    process
        .args([
            "--launch-development",
            "--allow-unisolated-backend",
            "--profile",
            "swift-pq2-assessment",
            "--data-directory",
        ])
        .arg(&config.data)
        .arg("--evidence-parent")
        .arg(&config.evidence)
        .arg("--frontend-port")
        .arg(config.frontend_port.to_string())
        .arg("--backend-port")
        .arg(config.backend_port.to_string())
        .arg("--max-waiting")
        .arg("0")
        .current_dir(&config.evidence)
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr));
    #[cfg(windows)]
    process.creation_flags(0x0800_0000);
    let mut broker = Broker(process.spawn().map_err(|_| {
        Failure::Message("Could not start the broker. Build the app and check its executable path.")
    })?);
    let key = wait_ready(&mut broker, config, commands, &stdout_path, &stderr_path)?;
    notify(events, Event::Ready)?;
    loop {
        if broker
            .0
            .try_wait()
            .map_err(|_| Failure::Message("Could not inspect the owned broker."))?
            .is_some()
        {
            let diagnostic = read_bounded_file(&stderr_path, MAX_HEAD).unwrap_or_default();
            return Err(runtime_failure(&diagnostic));
        }
        match commands.recv_timeout(POLL) {
            Ok(Command::Shutdown) | Err(RecvTimeoutError::Disconnected) => return Ok(()),
            Ok(Command::Stop) => return Err(Failure::Stopped),
            Ok(Command::NewChat) => history.clear(),
            Ok(Command::RestoreChat(restored)) => *history = restored,
            Ok(Command::Send {
                text,
                max_completion_tokens,
                extra_effort,
                enable_thinking,
                abliteration_scale,
            }) => {
                if text.trim().is_empty() {
                    continue;
                }
                let mut pending = history.clone();
                pending.push(Message {
                    role: "user",
                    content: text.clone(),
                    reasoning_content: None,
                });
                let scale = config.use_abliterate_lora.then_some(abliteration_scale);
                let outcome =
                    serialize_request(&pending, max_completion_tokens, enable_thinking, scale)
                        .and_then(|request| {
                            if extra_effort {
                                effort::run(
                                    config.frontend_port,
                                    &key,
                                    &pending,
                                    ResponseOptions {
                                        max_completion_tokens,
                                        enable_thinking,
                                        abliteration_scale: scale,
                                    },
                                    commands,
                                    events,
                                )
                            } else {
                                generate(config.frontend_port, &key, &request, commands, events)
                            }
                        });
                finish_turn(history, pending, &text, outcome, events, commands)?;
            }
            Ok(Command::Retry) | Err(RecvTimeoutError::Timeout) => {}
        }
    }
}

fn new_log(path: &Path) -> Result<File> {
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|_| {
            Failure::Message(
                "Could not create fresh diagnostic logs in the app evidence directory.",
            )
        })
}

fn wait_ready(
    broker: &mut Broker,
    config: &Config,
    commands: &Receiver<Command>,
    stdout_path: &Path,
    stderr_path: &Path,
) -> Result<String> {
    let until = Instant::now() + DEADLINE;
    loop {
        check_commands(commands)?;
        let output = read_bounded_file(stdout_path, MAX_HEAD)?;
        if let Some(directory) = evidence_directory(&output)? {
            let parent = config
                .evidence
                .canonicalize()
                .map_err(|_| Failure::Message("The app evidence directory is unavailable."))?;
            let directory = directory
                .canonicalize()
                .map_err(|_| Failure::Message("The broker credential directory is unavailable."))?;
            if directory == parent || !directory.starts_with(&parent) {
                return Err(Failure::Message(
                    "The broker reported an unexpected credential location.",
                ));
            }
            let key = read_bounded_file(&directory.join("frontend.key"), 64)?;
            if key.len() != 64
                || !key.iter().all(u8::is_ascii_hexdigit)
                || key.iter().any(u8::is_ascii_uppercase)
            {
                return Err(Failure::Message(
                    "The broker frontend credential has an invalid format.",
                ));
            }
            return String::from_utf8(key).map_err(|_| {
                Failure::Message("The broker frontend credential has an invalid format.")
            });
        }
        if broker
            .0
            .try_wait()
            .map_err(|_| Failure::Message("Could not inspect broker startup."))?
            .is_some()
        {
            let diagnostic = read_bounded_file(stderr_path, MAX_HEAD).unwrap_or_default();
            return Err(startup_failure(&diagnostic));
        }
        if Instant::now() >= until {
            return Err(Failure::Message(
                "The local model did not become ready within ten minutes.",
            ));
        }
        thread::sleep(POLL);
    }
}

fn startup_failure(diagnostic: &[u8]) -> Failure {
    let text = String::from_utf8_lossy(diagnostic);
    if text.contains("Memory") {
        Failure::Memory
    } else if text.contains("Port") || text.contains("Service") {
        Failure::Message(
            "A local chat port is unavailable. Close any other instance of this app before retrying the model.",
        )
    } else if text.contains("Adapter") {
        Failure::Message(
            "The pinned abliteration LoRA could not be verified. Run scripts/Get-Adapter.ps1 for this data directory, or restart with -NoAbliterateLora.",
        )
    } else if text.contains("Artifacts") {
        Failure::Message(
            "The pinned Swift model or CPU runtime could not be verified. Provision both with the Swift assessment lock.",
        )
    } else {
        Failure::Message("Swift could not start. Check the retained broker diagnostic logs.")
    }
}

fn runtime_failure(diagnostic: &[u8]) -> Failure {
    if String::from_utf8_lossy(diagnostic)
        .contains("Owned backend stopped after launcher failure: Memory.")
    {
        Failure::Message(RUNNING_MEMORY_MESSAGE)
    } else {
        Failure::Message("The model session ended. Select Retry model to continue.")
    }
}

fn read_bounded_file(path: &Path, limit: usize) -> Result<Vec<u8>> {
    let file = File::open(path)
        .map_err(|_| Failure::Message("A broker diagnostic or credential file is unavailable."))?;
    let mut bytes = Vec::new();
    file.take(
        u64::try_from(limit + 1)
            .map_err(|_| Failure::Message("The diagnostic limit is invalid."))?,
    )
    .read_to_end(&mut bytes)
    .map_err(|_| Failure::Message("Could not read a broker diagnostic or credential file."))?;
    if bytes.len() > limit {
        return Err(Failure::Message(
            "A broker diagnostic or credential file exceeded its limit.",
        ));
    }
    Ok(bytes)
}

fn evidence_directory(output: &[u8]) -> Result<Option<PathBuf>> {
    let Some(end) = output.iter().rposition(|byte| *byte == b'\n') else {
        return Ok(None);
    };
    let text = std::str::from_utf8(&output[..=end])
        .map_err(|_| Failure::Message("Broker startup output was not valid UTF-8."))?;
    // Only complete lines are considered: a poll may observe a partial write.
    for line in text
        .split_inclusive('\n')
        .filter(|line| line.ends_with('\n'))
    {
        if let Some(path) = line
            .trim_end_matches(['\r', '\n'])
            .strip_prefix(EVIDENCE_PREFIX)
        {
            let path = PathBuf::from(path);
            if !path.is_absolute() {
                return Err(Failure::Message(
                    "The broker reported a relative credential location.",
                ));
            }
            return Ok(Some(path));
        }
    }
    Ok(None)
}

fn finish_turn(
    history: &mut Vec<Message>,
    mut pending: Vec<Message>,
    text: &str,
    outcome: Result<(Message, Completion)>,
    events: &EventSender,
    commands: &Receiver<Command>,
) -> Result<()> {
    // Buffered parsing and local validation may finish without touching the
    // socket again. Stop takes precedence until this commit boundary.
    match check_commands(commands).and(outcome) {
        Ok((answer, completion)) => {
            pending.push(answer);
            *history = pending;
            notify(events, Event::Committed(history.clone()))?;
            notify(events, Event::Complete(completion))
        }
        Err(Failure::Shutdown) => Err(Failure::Shutdown),
        Err(Failure::Stopped) => {
            notify(events, Event::Stopping(text.to_owned()))?;
            Err(Failure::Stopped)
        }
        Err(failure) => {
            let message = match failure {
                Failure::Rejected(message) => message,
                Failure::Message(_) | Failure::Memory => {
                    "The response did not complete. Your message is restored for review; partial output has not been added to model history."
                }
                Failure::Shutdown | Failure::Stopped => return Err(failure),
            };
            notify(
                events,
                Event::Rejected {
                    message: message.to_owned(),
                    text: text.to_owned(),
                    // The UI must not enable Send during owned-process
                    // teardown; Retryable is sent only after Broker::drop.
                    retry_required: !matches!(failure, Failure::Rejected(_)),
                },
            )?;
            if matches!(failure, Failure::Rejected(_)) {
                Ok(())
            } else {
                Err(failure)
            }
        }
    }
}

fn serialize_request(
    history: &[Message],
    max_completion_tokens: u32,
    enable_thinking: bool,
    abliteration_scale: Option<f32>,
) -> Result<Vec<u8>> {
    if abliteration_scale.is_some_and(|scale| !valid_abliteration_scale(scale)) {
        return Err(Failure::Rejected(
            "Choose an abliteration scale between 0 and 2.",
        ));
    }
    if !(1..=4096).contains(&max_completion_tokens) {
        return Err(Failure::Rejected(
            "Choose an output budget between 1 and 4,096 tokens.",
        ));
    }
    if history.len() > 128 {
        return Err(Failure::Rejected(
            "This conversation reached the 128-message limit. Select New conversation before sending again.",
        ));
    }
    let body = serde_json::to_vec(&ChatRequest {
        messages: history,
        max_completion_tokens,
        stream: true,
        enable_thinking,
        abliteration_scale,
    })
    .map_err(|_| Failure::Message("Could not prepare the chat request."))?;
    if body.len() > MAX_REQUEST {
        return Err(Failure::Rejected(
            "This conversation exceeded the request byte limit. Shorten your message or select New conversation.",
        ));
    }
    Ok(body)
}

fn notify(events: &EventSender, event: Event) -> Result<()> {
    events.send(event).map_err(|()| Failure::Shutdown)
}

fn check_commands(commands: &Receiver<Command>) -> Result<()> {
    loop {
        match commands.try_recv() {
            Ok(Command::Shutdown) | Err(TryRecvError::Disconnected) => {
                return Err(Failure::Shutdown);
            }
            Ok(Command::Stop) => return Err(Failure::Stopped),
            Ok(
                Command::Send { .. } | Command::NewChat | Command::RestoreChat(_) | Command::Retry,
            ) => {}
            Err(TryRecvError::Empty) => return Ok(()),
        }
    }
}

struct Interruptible<'a> {
    stream: TcpStream,
    commands: &'a Receiver<Command>,
    until: Instant,
    interrupted: Option<Failure>,
}

impl Interruptible<'_> {
    fn check(&mut self) -> io::Result<()> {
        if self.interrupted.is_some() {
            return Err(io::Error::new(
                io::ErrorKind::ConnectionAborted,
                "Native chat request interrupted",
            ));
        }
        let result = check_commands(self.commands).and_then(|()| {
            if Instant::now() >= self.until {
                Err(Failure::Message(
                    "The local model took longer than the ten-minute request limit.",
                ))
            } else {
                Ok(())
            }
        });
        if let Err(failure) = result {
            self.interrupted = Some(failure);
            return Err(io::Error::new(
                io::ErrorKind::ConnectionAborted,
                "Native chat request interrupted",
            ));
        }
        Ok(())
    }

    fn wait(&mut self) -> io::Result<()> {
        // The socket is nonblocking: wait on the command channel instead of
        // a socket timeout, so Stop/Shutdown wake a stalled read or write.
        let timeout = self
            .until
            .saturating_duration_since(Instant::now())
            .min(IO_POLL);
        match self.commands.recv_timeout(timeout) {
            Ok(Command::Stop) => self.interrupted = Some(Failure::Stopped),
            Ok(Command::Shutdown) | Err(RecvTimeoutError::Disconnected) => {
                self.interrupted = Some(Failure::Shutdown);
            }
            Ok(_) | Err(RecvTimeoutError::Timeout) => {}
        }
        self.check()
    }
}

impl Read for Interruptible<'_> {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        loop {
            self.check()?;
            match self.stream.read(bytes) {
                Err(error)
                    if matches!(
                        error.kind(),
                        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                    ) =>
                {
                    self.wait()?;
                }
                result => {
                    self.check()?;
                    return result;
                }
            }
        }
    }
}

impl Write for Interruptible<'_> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        loop {
            self.check()?;
            match self.stream.write(bytes) {
                Err(error)
                    if matches!(
                        error.kind(),
                        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                    ) =>
                {
                    self.wait()?;
                }
                result => {
                    self.check()?;
                    return result;
                }
            }
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        self.stream.flush()
    }
}

fn generate(
    port: u16,
    key: &str,
    request: &[u8],
    commands: &Receiver<Command>,
    events: &EventSender,
) -> Result<(Message, Completion)> {
    generate_until(
        port,
        key,
        request,
        commands,
        events,
        Instant::now() + DEADLINE,
    )
}

fn generate_until(
    port: u16,
    key: &str,
    request: &[u8],
    commands: &Receiver<Command>,
    events: &EventSender,
    until: Instant,
) -> Result<(Message, Completion)> {
    check_commands(commands)?;
    if Instant::now() >= until {
        return Err(Failure::Message(
            "The response exceeded its overall ten-minute deadline.",
        ));
    }
    let address = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, port));
    let connect_timeout = until
        .saturating_duration_since(Instant::now())
        .min(Duration::from_secs(1));
    let connected = TcpStream::connect_timeout(&address, connect_timeout);
    check_commands(commands)?;
    let stream = connected.map_err(|_| {
        Failure::Message("Could not connect to the model service on this computer.")
    })?;
    stream
        .set_nonblocking(true)
        .map_err(|_| Failure::Message("Could not configure the local connection."))?;
    let mut connection = Interruptible {
        stream,
        commands,
        until,
        interrupted: None,
    };
    let outcome = exchange(&mut connection, port, key, request, events);
    let _checked = connection.check();
    connection.interrupted.map_or(outcome, Err)
}

fn exchange(
    connection: &mut Interruptible<'_>,
    port: u16,
    key: &str,
    request: &[u8],
    events: &EventSender,
) -> Result<(Message, Completion)> {
    let header = format!(
        "POST /v1/chat/completions HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAuthorization: Bearer {key}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        request.len()
    );
    // Keep the sending half open: the broker interprets half-close as abandonment.
    connection
        .write_all(header.as_bytes())
        .and_then(|()| connection.write_all(request))
        .map_err(|_| {
            Failure::Message("Could not send your message to the model on this computer.")
        })?;
    let commands = connection.commands;
    let until = connection.until;
    let mut body = HttpBody::new(connection)?;
    let mut sse = Sse::default();
    let mut bytes = [0; READ_SIZE];
    while let Some(count) = body.next(&mut bytes)? {
        sse.push(&bytes[..count], &mut |text, reasoning| {
            if !text.is_empty() {
                events.stream_until(Event::Text(text), commands, until)?;
            }
            if !reasoning.is_empty() {
                events.stream_until(Event::Reasoning(reasoning), commands, until)?;
            }
            Ok(())
        })?;
    }
    let completion = sse.finish()?;
    Ok((
        Message {
            role: "assistant",
            content: sse.text,
            reasoning_content: (!sse.reasoning.is_empty()).then_some(sse.reasoning),
        },
        completion,
    ))
}

#[derive(Clone, Copy)]
enum Framing {
    Chunked,
    Length(usize),
}

#[derive(Clone, Copy)]
enum ResponseKind {
    Stream,
    ContextError,
}

struct HttpBody<R> {
    reader: BufReader<R>,
    framing: Framing,
    remaining: usize,
    total: usize,
    done: bool,
    chunk_ending: bool,
}

impl<R: Read> HttpBody<R> {
    fn new(reader: R) -> Result<Self> {
        let mut reader = BufReader::new(reader);
        let mut head = Vec::new();
        while !head.ends_with(b"\r\n\r\n") {
            if head.len() == MAX_HEAD {
                return Err(Failure::Message(
                    "The local response header exceeded its limit.",
                ));
            }
            head.push(read_byte(&mut reader)?);
        }
        let (framing, kind) = parse_head(&head)?;
        let mut body = Self {
            reader,
            framing,
            remaining: 0,
            total: 0,
            done: false,
            chunk_ending: false,
        };
        if matches!(kind, ResponseKind::ContextError) {
            return Err(body.context_error()?);
        }
        Ok(body)
    }

    fn context_error(&mut self) -> Result<Failure> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Envelope {
            error: ErrorCode,
        }
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct ErrorCode {
            code: String,
        }
        let invalid = Failure::Message("The local model returned an unrecognized error reply.");
        let mut error = Vec::new();
        let mut bytes = [0; READ_SIZE];
        while let Some(count) = self.next(&mut bytes)? {
            if count > MAX_ERROR_BODY - error.len() {
                return Err(invalid);
            }
            error.extend_from_slice(&bytes[..count]);
        }
        let envelope: Envelope = serde_json::from_slice(&error).map_err(|_| invalid)?;
        if envelope.error.code != "context_length_exceeded" {
            return Err(invalid);
        }
        Ok(Failure::Rejected(CONTEXT_LIMIT_MESSAGE))
    }

    fn next(&mut self, bytes: &mut [u8; READ_SIZE]) -> Result<Option<usize>> {
        if self.done {
            return Ok(None);
        }
        match self.framing {
            Framing::Length(length) => {
                let count = READ_SIZE.min(length - self.total);
                if count == 0 {
                    self.done = true;
                    return Ok(None);
                }
                let count = read_payload(&mut self.reader, &mut bytes[..count])?;
                self.total += count;
                Ok(Some(count))
            }
            Framing::Chunked => self.chunk(bytes),
        }
    }

    fn chunk(&mut self, bytes: &mut [u8; READ_SIZE]) -> Result<Option<usize>> {
        if self.remaining == 0 {
            if self.chunk_ending {
                let mut ending = [0; 2];
                read_exact(&mut self.reader, &mut ending)?;
                if ending != *b"\r\n" {
                    return Err(Failure::Message(
                        "The local response had invalid chunk endings.",
                    ));
                }
                self.chunk_ending = false;
            }
            let mut line = [0; 32];
            let line = read_line(&mut self.reader, &mut line)?;
            if line.is_empty() || !line.iter().all(u8::is_ascii_hexdigit) {
                return Err(Failure::Message(
                    "The local response had invalid chunk framing.",
                ));
            }
            let text = std::str::from_utf8(line)
                .map_err(|_| Failure::Message("The local chunk size was invalid."))?;
            self.remaining = usize::from_str_radix(text, 16)
                .map_err(|_| Failure::Message("The local chunk size was invalid."))?;
            if self.remaining > MAX_BODY - self.total {
                return Err(Failure::Message(
                    "The response exceeded its four-megabyte limit.",
                ));
            }
            if self.remaining == 0 {
                if !read_line(&mut self.reader, &mut [0; 2])?.is_empty() {
                    return Err(Failure::Message("Response trailers are unsupported."));
                }
                self.done = true;
                return Ok(None);
            }
        }
        let count = READ_SIZE.min(self.remaining);
        let count = read_payload(&mut self.reader, &mut bytes[..count])?;
        self.remaining -= count;
        self.total += count;
        // Deliver ready payload before waiting for its trailing framing bytes.
        // The next call must validate them before reading another size or EOF.
        self.chunk_ending = self.remaining == 0;
        Ok(Some(count))
    }
}

fn read_payload(reader: &mut impl Read, bytes: &mut [u8]) -> Result<usize> {
    // One successful read releases its prefix immediately. read_exact here would
    // hold a complete SSE event until the rest of an HTTP chunk arrived.
    // Interrupted remains visible, preserving the cancellation contract below.
    let count = reader.read(bytes).map_err(|_| {
        Failure::Message("The connection to the model on this computer was interrupted before a complete reply was received.")
    })?;
    if count == 0 {
        return Err(Failure::Message(LOCAL_CONNECTION_CLOSED));
    }
    Ok(count)
}

fn read_exact(reader: &mut impl Read, bytes: &mut [u8]) -> Result<()> {
    // Do not use Read::read_exact: it retries Interrupted and could hide a
    // Shutdown/Stop request from the interruptible socket until another read.
    let mut filled = 0;
    while filled < bytes.len() {
        let count = reader.read(&mut bytes[filled..]).map_err(|_| {
            Failure::Message("The connection to the model on this computer was interrupted before a complete reply was received.")
        })?;
        if count == 0 {
            return Err(Failure::Message(LOCAL_CONNECTION_CLOSED));
        }
        filled += count;
    }
    Ok(())
}

fn read_byte(reader: &mut impl Read) -> Result<u8> {
    let mut byte = [0];
    read_exact(reader, &mut byte)?;
    Ok(byte[0])
}

fn read_line<'a>(reader: &mut impl Read, bytes: &'a mut [u8]) -> Result<&'a [u8]> {
    let mut length = 0;
    loop {
        if length == bytes.len() {
            return Err(Failure::Message(
                "The local model returned reply metadata larger than the app can accept.",
            ));
        }
        bytes[length] = read_byte(reader)?;
        length += 1;
        if bytes[..length].ends_with(b"\r\n") {
            return Ok(&bytes[..length - 2]);
        }
    }
}

fn parse_head(head: &[u8]) -> Result<(Framing, ResponseKind)> {
    let mut headers = [httparse::EMPTY_HEADER; 64];
    let mut response = httparse::Response::new(&mut headers);
    if !matches!(response.parse(head), Ok(httparse::Status::Complete(_)))
        || response.version != Some(1)
    {
        return Err(Failure::Message(
            "The model on this computer returned a reply the app could not read.",
        ));
    }
    let kind = match response.code {
        Some(200) => ResponseKind::Stream,
        Some(413) => ResponseKind::ContextError,
        _ => {
            return Err(Failure::Message(
                "The local model rejected the request without reporting a recognized reason.",
            ));
        }
    };
    let mut content_type = None;
    let mut length = None;
    let mut transfer = None;
    for header in response.headers {
        let field = if header.name.eq_ignore_ascii_case("content-type") {
            Some(&mut content_type)
        } else if header.name.eq_ignore_ascii_case("content-length") {
            Some(&mut length)
        } else if header.name.eq_ignore_ascii_case("transfer-encoding") {
            Some(&mut transfer)
        } else {
            None
        };
        if let Some(field) = field
            && field.replace(header.value).is_some()
        {
            return Err(Failure::Message(
                "The local response repeated a framing header.",
            ));
        }
        if header.name.eq_ignore_ascii_case("content-encoding") {
            return Err(Failure::Message("Encoded local responses are unsupported."));
        }
    }
    let valid_media = match kind {
        ResponseKind::Stream => content_type.is_some_and(is_event_stream),
        ResponseKind::ContextError => {
            content_type.is_some_and(|value| value.eq_ignore_ascii_case(b"application/json"))
        }
    };
    if !valid_media {
        return Err(Failure::Message(
            "The local model returned a reply format the app could not read.",
        ));
    }
    let framing = match (length, transfer) {
        (None, Some(value)) if value.eq_ignore_ascii_case(b"chunked") => Framing::Chunked,
        (Some(value), None) if !value.is_empty() && value.iter().all(u8::is_ascii_digit) => {
            let length = std::str::from_utf8(value)
                .ok()
                .and_then(|text| text.parse::<usize>().ok())
                .filter(|length| {
                    *length
                        <= match kind {
                            ResponseKind::Stream => MAX_BODY,
                            ResponseKind::ContextError => MAX_ERROR_BODY,
                        }
                })
                .ok_or(Failure::Message(
                    "The response length is invalid or exceeded its limit.",
                ))?;
            Framing::Length(length)
        }
        _ => {
            return Err(Failure::Message(
                "The local response had unsupported or conflicting framing.",
            ));
        }
    };
    Ok((framing, kind))
}

fn is_event_stream(value: &[u8]) -> bool {
    let Ok(value) = std::str::from_utf8(value) else {
        return false;
    };
    let mut parts = value.split(';');
    if !parts
        .next()
        .is_some_and(|media| media.trim().eq_ignore_ascii_case("text/event-stream"))
    {
        return false;
    }
    let Some(parameter) = parts.next() else {
        return true;
    };
    let Some((name, charset)) = parameter.trim().split_once('=') else {
        return false;
    };
    name.trim().eq_ignore_ascii_case("charset")
        && (charset.trim().eq_ignore_ascii_case("utf-8")
            || charset.trim().eq_ignore_ascii_case("\"utf-8\""))
        && parts.next().is_none()
}

#[derive(Default)]
struct Sse {
    initial: Vec<u8>,
    started: bool,
    after_cr: bool,
    line: Vec<u8>,
    data: Vec<u8>,
    event: Vec<u8>,
    text: String,
    reasoning: String,
    completion: Option<Completion>,
    done: bool,
}

#[derive(Deserialize)]
struct Chunk {
    #[serde(default)]
    choices: Vec<Choice>,
    error: Option<serde_json::Value>,
}
#[derive(Deserialize)]
struct Choice {
    #[serde(default)]
    delta: Delta,
    index: Option<u32>,
    finish_reason: Option<String>,
}
#[derive(Default, Deserialize)]
struct Delta {
    content: Option<String>,
    reasoning_content: Option<String>,
    reasoning: Option<String>,
}

impl Sse {
    fn push(
        &mut self,
        bytes: &[u8],
        delta: &mut impl FnMut(String, String) -> Result<()>,
    ) -> Result<()> {
        for &byte in bytes {
            if !self.started {
                self.initial.push(byte);
                if b"\xEF\xBB\xBF".starts_with(&self.initial) {
                    if self.initial.len() < 3 {
                        continue;
                    }
                    self.initial.clear();
                    self.started = true;
                    continue;
                }
                self.started = true;
                for initial in std::mem::take(&mut self.initial) {
                    self.push_byte(initial, delta)?;
                }
                continue;
            }
            self.push_byte(byte, delta)?;
        }
        Ok(())
    }

    fn push_byte(
        &mut self,
        byte: u8,
        delta: &mut impl FnMut(String, String) -> Result<()>,
    ) -> Result<()> {
        if self.after_cr {
            self.after_cr = false;
            if byte == b'\n' {
                return Ok(());
            }
        }
        if matches!(byte, b'\r' | b'\n') {
            self.process_line(delta)?;
            self.after_cr = byte == b'\r';
        } else {
            if self.line.len() == MAX_EVENT {
                return Err(Failure::Message("A streaming line exceeded its limit."));
            }
            self.line.push(byte);
        }
        Ok(())
    }

    fn process_line(&mut self, delta: &mut impl FnMut(String, String) -> Result<()>) -> Result<()> {
        if self.line.is_empty() {
            self.dispatch(delta)?;
            self.event.clear();
        } else if self.line.first() != Some(&b':') {
            let (field, value) = self.line.iter().position(|&byte| byte == b':').map_or(
                (self.line.as_slice(), b"".as_slice()),
                |colon| {
                    let value = &self.line[colon + 1..];
                    (
                        &self.line[..colon],
                        value.strip_prefix(b" ").unwrap_or(value),
                    )
                },
            );
            match field {
                b"data" => {
                    if self.data.len() + value.len() + 1 > MAX_EVENT {
                        return Err(Failure::Message("A streaming event exceeded its limit."));
                    }
                    self.data.extend_from_slice(value);
                    self.data.push(b'\n');
                }
                b"event" => {
                    self.event.clear();
                    self.event.extend_from_slice(value);
                }
                // id/retry affect reconnection only. Requests are never replayed:
                // the owned broker treats interruption as a cancelled generation.
                _ => {}
            }
        }
        self.line.clear();
        Ok(())
    }

    fn dispatch(&mut self, delta: &mut impl FnMut(String, String) -> Result<()>) -> Result<()> {
        if self.data.is_empty() {
            return Ok(());
        }
        self.data.pop();
        if self.event == b"error" {
            return Err(Failure::Message("The model reported a streaming error."));
        }
        if !self.event.is_empty() && self.event != b"message" {
            return Err(Failure::Message(
                "The model returned an unsupported event type.",
            ));
        }
        if self.data == b"[DONE]" {
            if self.done || self.completion.is_none() {
                return Err(Failure::Message(
                    "The response had an invalid terminal completion marker.",
                ));
            }
            self.done = true;
        } else {
            if self.done {
                return Err(Failure::Message(
                    "The response continued after its completion marker.",
                ));
            }
            let chunk: Chunk = serde_json::from_slice(&self.data)
                .map_err(|_| Failure::Message("The model returned an invalid streaming event."))?;
            if chunk.error.is_some() || chunk.choices.len() > 1 {
                return Err(Failure::Message(
                    "The model could not complete this response.",
                ));
            }
            if let Some(choice) = chunk.choices.into_iter().next() {
                if self.completion.is_some() || choice.index.is_some_and(|index| index != 0) {
                    return Err(Failure::Message(
                        "The model returned an unexpected completion choice.",
                    ));
                }
                let text = choice.delta.content.unwrap_or_default();
                let reasoning = choice
                    .delta
                    .reasoning_content
                    .or(choice.delta.reasoning)
                    .unwrap_or_default();
                self.text.push_str(&text);
                self.reasoning.push_str(&reasoning);
                delta(text, reasoning)?;
                if let Some(reason) = choice.finish_reason {
                    self.completion = Some(match reason.as_str() {
                        "stop" => Completion::Finished,
                        "length" => Completion::TokenLimit,
                        "content_filter" => {
                            return Err(Failure::Message("The model filtered this response."));
                        }
                        _ => {
                            return Err(Failure::Message(
                                "The model ended with an unsupported completion reason.",
                            ));
                        }
                    });
                }
            }
        }
        self.data.clear();
        Ok(())
    }

    fn finish(&self) -> Result<Completion> {
        if !self.done {
            return Err(Failure::Message(
                "The local model ended its reply without confirming that generation finished.",
            ));
        }
        if self.text.is_empty() && self.reasoning.is_empty() {
            return Err(Failure::Message("The local model returned no text."));
        }
        // SSE discards any final event lacking a blank-line terminator. The
        // explicit terminal event and [DONE] above must both have dispatched.
        self.completion.ok_or(Failure::Message(
            "The response omitted its terminal completion reason.",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::{
        Command, Completion, Event, Failure, HttpBody, Message, READ_SIZE, Sse, evidence_directory,
        finish_turn, parse_head, serialize_request, startup_failure, wait_for_retry,
    };

    #[test]
    fn thinking_request_selection_preserves_supplied_reasoning() -> Result<(), Failure> {
        let history = [
            Message {
                role: "user",
                content: "Write a garden scene.".to_owned(),
                reasoning_content: None,
            },
            Message {
                role: "assistant",
                content: "The garden grew quiet.".to_owned(),
                reasoning_content: Some("Retained prior reasoning".to_owned()),
            },
            Message {
                role: "user",
                content: "Continue.".to_owned(),
                reasoning_content: None,
            },
        ];
        for enable_thinking in [false, true] {
            let body = serialize_request(&history, 4096, enable_thinking, None)?;
            let request: serde_json::Value = serde_json::from_slice(&body)
                .map_err(|_| Failure::Message("invalid thinking request JSON"))?;
            assert_eq!(request["enable_thinking"], enable_thinking);
            assert_eq!(
                request["messages"][1]["reasoning_content"],
                "Retained prior reasoning"
            );
            assert_eq!(request["max_completion_tokens"], 4096);
            assert_eq!(request["stream"], true);
        }
        Ok(())
    }

    #[test]
    fn abliteration_requests_preserve_scale_bounds_and_omit_disabled_adapter() -> Result<(), Failure>
    {
        let history = [Message {
            role: "user",
            content: "Write a garden scene.".to_owned(),
            reasoning_content: None,
        }];
        for scale in [0.0, super::DEFAULT_ABLITERATION_SCALE, 2.0] {
            let body = serialize_request(&history, 4096, false, Some(scale))?;
            let request: serde_json::Value = serde_json::from_slice(&body)
                .map_err(|_| Failure::Message("invalid scale request JSON"))?;
            assert!(request["abliteration_scale"].as_f64().is_some_and(|value| {
                (value - f64::from(scale)).abs() < f64::from(f32::EPSILON)
            }));
            assert!(request.get("lora").is_none());
        }
        let body = serialize_request(&history, 4096, false, None)?;
        let request: serde_json::Value = serde_json::from_slice(&body)
            .map_err(|_| Failure::Message("invalid disabled request JSON"))?;
        assert!(request.get("abliteration_scale").is_none());
        for scale in [-0.1, 2.1, f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            assert!(matches!(
                serialize_request(&history, 4096, false, Some(scale)),
                Err(Failure::Rejected(_))
            ));
        }
        Ok(())
    }
    use std::io::{self, Read};

    struct Fragmented<'a>(&'a [u8]);
    impl Read for Fragmented<'_> {
        fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
            let count = usize::from(!self.0.is_empty()).min(bytes.len());
            bytes[..count].copy_from_slice(&self.0[..count]);
            self.0 = &self.0[count..];
            Ok(count)
        }
    }

    #[test]
    fn conversation_workspace_restores_committed_history_during_retry_without_replay()
    -> Result<(), Failure> {
        let bytes = br#"[{"role":"user","content":"Saved question"},{"role":"assistant","content":"Saved answer","reasoning_content":"Saved reasoning"}]"#;
        let saved: Vec<Message> = serde_json::from_slice(bytes)
            .map_err(|_| Failure::Message("History roundtrip failed"))?;
        let expected = serialize_request(&saved, 4096, false, None)?;
        assert!(
            serde_json::from_slice::<Message>(br#"{"role":"system","content":"Unaccepted role"}"#)
                .is_err()
        );
        let (commands, incoming) = std::sync::mpsc::channel();
        let (events, notifications) = crate::client::event_channel();
        assert!(commands.send(Command::RestoreChat(saved)).is_ok());
        assert!(commands.send(Command::Retry).is_ok());
        let mut history = Vec::new();
        assert!(wait_for_retry(
            &incoming,
            &events,
            &mut history,
            &Event::Retryable("Retry explicitly.".to_owned())
        ));
        assert_eq!(serialize_request(&history, 4096, false, None)?, expected);
        assert!(matches!(notifications.try_recv(), Ok(Event::Retryable(_))));
        assert!(notifications.try_recv().is_err());
        Ok(())
    }
    #[test]
    fn fragmented_chunked_unicode_stream_preserves_reasoning_and_history() -> Result<(), Failure> {
        let events = concat!(
            "data: {\"choices\":[{\"delta\":{\"reasoning_content\":\"Consider 🌱\"}}]}\r\n\r\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\"Rain 雨\"}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
            "data: [DONE]\n\n"
        );
        let mut wire = b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\n\r\n".to_vec();
        for chunk in events.as_bytes().chunks(7) {
            wire.extend_from_slice(format!("{:X}\r\n", chunk.len()).as_bytes());
            wire.extend_from_slice(chunk);
            wire.extend_from_slice(b"\r\n");
        }
        wire.extend_from_slice(b"0\r\n\r\n");
        let mut body = HttpBody::new(Fragmented(&wire))?;
        let mut sse = Sse::default();
        let mut text = String::new();
        let mut reasoning = String::new();
        let mut bytes = [0; READ_SIZE];
        while let Some(count) = body.next(&mut bytes)? {
            sse.push(&bytes[..count], &mut |content, thought| {
                text.push_str(&content);
                reasoning.push_str(&thought);
                Ok(())
            })?;
        }
        sse.finish()?;
        assert_eq!(text, "Rain 雨");
        assert_eq!(reasoning, "Consider 🌱");
        let request = serialize_request(
            &[
                Message {
                    role: "user",
                    content: "Begin".into(),
                    reasoning_content: None,
                },
                Message {
                    role: "assistant",
                    content: text,
                    reasoning_content: Some(reasoning),
                },
                Message {
                    role: "user",
                    content: "Continue".into(),
                    reasoning_content: None,
                },
            ],
            4096,
            false,
            None,
        )?;
        let json: serde_json::Value =
            serde_json::from_slice(&request).map_err(|_| Failure::Message("test request JSON"))?;
        assert_eq!(json["messages"][1]["reasoning_content"], "Consider 🌱");
        assert!(json["messages"][0].get("reasoning_content").is_none());
        assert_eq!(json["max_completion_tokens"], 4096);
        assert_eq!(json["stream"], true);
        assert_eq!(json.as_object().map(serde_json::Map::len), Some(4));
        Ok(())
    }

    #[test]
    fn http_body_preserves_short_final_reads_for_both_framings() -> Result<(), Failure> {
        let mut expected = vec![b'x'; READ_SIZE + 3];
        expected[READ_SIZE..].copy_from_slice(b"end");
        for chunked in [false, true] {
            let framing = if chunked {
                format!("Transfer-Encoding: chunked\r\n\r\n{:X}\r\n", expected.len())
            } else {
                format!("Content-Length: {}\r\n\r\n", expected.len())
            };
            let mut wire = b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\n".to_vec();
            wire.extend_from_slice(framing.as_bytes());
            wire.extend_from_slice(&expected);
            if chunked {
                wire.extend_from_slice(b"\r\n0\r\n\r\n");
            }
            let mut body = HttpBody::new(Fragmented(&wire))?;
            let mut bytes = [0; READ_SIZE];
            let mut actual = Vec::new();
            while let Some(count) = body.next(&mut bytes)? {
                actual.extend_from_slice(&bytes[..count]);
            }
            assert_eq!(actual, expected);
            assert_eq!(body.next(&mut bytes)?, None);
        }
        Ok(())
    }

    struct ReadyPrefix<'a>(&'a [u8]);

    impl Read for ReadyPrefix<'_> {
        fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
            if self.0.is_empty() {
                return Err(io::Error::from(io::ErrorKind::WouldBlock));
            }
            self.0.read(bytes)
        }
    }

    #[test]
    fn http_payload_releases_ready_sse_before_the_declared_body_arrives() -> Result<(), Failure> {
        let event = b"data: {\"choices\":[{\"delta\":{\"content\":\"Ready\"}}]}\n\n";
        for framing in [
            "Content-Length: 8192\r\n\r\n",
            "Transfer-Encoding: chunked\r\n\r\n2000\r\n",
        ] {
            let mut wire = b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\n".to_vec();
            wire.extend_from_slice(framing.as_bytes());
            wire.extend_from_slice(event);
            let mut body = HttpBody::new(ReadyPrefix(&wire))?;
            let mut bytes = [0; READ_SIZE];
            assert_eq!(body.next(&mut bytes)?, Some(event.len()));
            assert_eq!(&bytes[..event.len()], event);
            let mut received = String::new();
            Sse::default().push(&bytes[..event.len()], &mut |text, _| {
                received.push_str(&text);
                Ok(())
            })?;
            assert_eq!(received, "Ready");
            // The gate rejects a second underlying read. It was not needed to
            // publish the complete event, and the unfinished body cannot pass.
            assert!(body.next(&mut bytes).is_err());
        }
        Ok(())
    }

    #[test]
    fn http_payload_precedes_chunk_ending_but_invalid_endings_never_complete() -> Result<(), Failure>
    {
        for ending in [b"".as_slice(), b"\r", b"xx", b"\r\n0\r\n\r"] {
            let mut wire = b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\n\r\n3\r\nabc".to_vec();
            wire.extend_from_slice(ending);
            let mut body = HttpBody::new(ReadyPrefix(&wire))?;
            let mut bytes = [0; READ_SIZE];
            assert_eq!(body.next(&mut bytes)?, Some(3));
            assert_eq!(&bytes[..3], b"abc");
            assert!(body.next(&mut bytes).is_err());
        }
        Ok(())
    }

    #[test]
    fn http_payload_preserves_interrupted_reads_and_bounded_chunk_metadata() -> Result<(), Failure>
    {
        struct Interrupted;
        impl Read for Interrupted {
            fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
                Err(io::Error::from(io::ErrorKind::Interrupted))
            }
        }
        let head =
            b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: 1\r\n\r\n";
        let mut body = HttpBody::new(head.as_slice().chain(Interrupted))?;
        assert!(body.next(&mut [0; READ_SIZE]).is_err());
        for chunk in [
            "400001\r\n", // The declared payload exceeds the body budget.
            "1;extension\r\n",
            "0000000000000000000000000000001\r\n", // Beyond 32 bytes including CRLF.
            "0\r\nX-Trailer: no\r\n\r\n",
        ] {
            let wire = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\n\r\n{chunk}"
            );
            let mut body = HttpBody::new(wire.as_bytes())?;
            assert!(body.next(&mut [0; READ_SIZE]).is_err());
        }
        let wire = b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\n\r\n000000000000000000000000000001\r\nx\r\n0\r\n\r\n";
        let mut body = HttpBody::new(wire.as_slice())?;
        let mut bytes = [0; READ_SIZE];
        assert_eq!(body.next(&mut bytes)?, Some(1));
        assert_eq!(bytes[0], b'x');
        assert_eq!(body.next(&mut bytes)?, None);
        Ok(())
    }

    #[test]
    fn truncated_http_and_incomplete_generation_are_rejected() -> Result<(), Failure> {
        let mut body = HttpBody::new(Fragmented(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\n\r\n5\r\nabc"))?;
        let mut received = Vec::new();
        loop {
            let mut bytes = [0; READ_SIZE];
            match body.next(&mut bytes) {
                Ok(Some(count)) => received.extend_from_slice(&bytes[..count]),
                Err(_) => break,
                Ok(None) => return Err(Failure::Message("Truncated HTTP was accepted")),
            }
        }
        assert_eq!(received, b"abc");
        let mut sse = Sse::default();
        sse.push(
            b"data: {\"choices\":[{\"delta\":{\"content\":\"partial\"}}]}\n\n",
            &mut |_, _| Ok(()),
        )?;
        assert!(sse.finish().is_err());
        assert!(parse_head(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: 1\r\nTransfer-Encoding: chunked\r\n\r\n").is_err());
        assert!(parse_head(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: 1\r\nContent-Length: 1\r\n\r\n").is_err());
        Ok(())
    }

    fn context_response(body: &[u8], chunked: bool) -> Vec<u8> {
        let mut wire =
            b"HTTP/1.1 413 Content Too Large\r\nContent-Type: application/json\r\n".to_vec();
        if chunked {
            wire.extend_from_slice(
                format!("Transfer-Encoding: chunked\r\n\r\n{:X}\r\n", body.len()).as_bytes(),
            );
            wire.extend_from_slice(body);
            wire.extend_from_slice(b"\r\n0\r\n\r\n");
        } else {
            wire.extend_from_slice(format!("Content-Length: {}\r\n\r\n", body.len()).as_bytes());
            wire.extend_from_slice(body);
        }
        wire
    }

    #[test]
    fn context_rejection_requires_a_complete_bounded_recognized_reply() {
        let body = br#"{"error":{"code":"context_length_exceeded"}}"#;
        for chunked in [false, true] {
            let wire = context_response(body, chunked);
            assert!(
                matches!(HttpBody::new(Fragmented(&wire)), Err(Failure::Rejected(message))
                if message == super::CONTEXT_LIMIT_MESSAGE)
            );
            // Even a complete JSON code is insufficient without the complete
            // declared body or final chunk terminator.
            assert!(matches!(
                HttpBody::new(Fragmented(&wire[..wire.len() - 1])),
                Err(Failure::Message(_))
            ));
        }
        for invalid in [
            br#"{"error":{"code":"unknown"}}"#.as_slice(),
            br#"{"error":{"code":"context_length_exceeded","message":"untrusted text"}}"#,
            br#"{"error":{"code":"context_length_exceeded","code":"context_length_exceeded"}}"#,
            b"not JSON",
            &[b'x'; super::MAX_ERROR_BODY + 1],
        ] {
            for chunked in [false, true] {
                let wire = context_response(invalid, chunked);
                assert!(matches!(
                    HttpBody::new(Fragmented(&wire)),
                    Err(Failure::Message(_))
                ));
            }
        }
        for headers in [
            "Content-Length: 44\r\nContent-Length: 44",
            "Content-Length: 44\r\nTransfer-Encoding: chunked",
            "Content-Length: 44\r\nContent-Encoding: gzip",
        ] {
            let wire = format!(
                "HTTP/1.1 413 Content Too Large\r\nContent-Type: application/json\r\n{headers}\r\n\r\n"
            );
            assert!(matches!(
                HttpBody::new(Fragmented(wire.as_bytes())),
                Err(Failure::Message(_))
            ));
        }
        let wire = context_response(body, false);
        let wrong_status = String::from_utf8_lossy(&wire)
            .replace("413 Content Too Large", "500 Internal Server Error");
        assert!(matches!(
            HttpBody::new(Fragmented(wrong_status.as_bytes())),
            Err(Failure::Message(_))
        ));
    }

    #[test]
    fn unexpected_disconnect_explains_local_connection_without_guessing_token_limits() {
        let wire = b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\n";
        for response in [b"".as_slice(), wire] {
            assert!(
                matches!(HttpBody::new(Fragmented(response)), Err(Failure::Message(message))
                if message == super::LOCAL_CONNECTION_CLOSED)
            );
        }
        let guidance = super::retry_guidance(super::LOCAL_CONNECTION_CLOSED);
        assert!(guidance.contains("on this computer"));
        assert!(guidance.contains("cause was not reported"));
        assert!(guidance.contains("Retry model"));
        for misleading in [
            "HTTP",
            "Restart the app",
            "output budget",
            "request was too long",
        ] {
            assert!(!guidance.contains(misleading));
        }
    }

    #[test]
    fn event_stream_conformance_handles_bom_cr_lines_and_split_fields() -> Result<(), Failure> {
        for ending in ["\r", "\n", "\r\n"] {
            let events = [
                ": keepalive",
                "id: ignored-without-reconnect",
                "retry: 1000",
                "event: message",
                // A colonless field is a field whose value is empty. The data
                // lines concatenate with LF before JSON decoding.
                "data",
                "data: {\"choices\":[{\"delta\":",
                "data: {\"content\":\"Rain 雨\"},\"index\":0,\"finish_reason\":\"stop\"}]}",
                "",
                "data: [DONE]",
                "",
                ": unterminated comment is discarded at EOF",
            ]
            .join(ending);
            let mut wire = b"\xEF\xBB\xBF".to_vec();
            wire.extend_from_slice(events.as_bytes());
            let mut sse = Sse::default();
            let mut text = String::new();
            for byte in wire {
                sse.push(&[byte], &mut |content, _| {
                    text.push_str(&content);
                    Ok(())
                })?;
            }
            assert_eq!(sse.finish()?, Completion::Finished);
            assert_eq!(text, "Rain 雨");
        }
        Ok(())
    }

    #[test]
    fn terminal_reasons_distinguish_token_limits_and_reject_false_success() -> Result<(), Failure> {
        let mut capped = Sse::default();
        capped.push(
            concat!(
                "data: {\"choices\":[{\"delta\":{\"content\":\"Partial answer\"},\"finish_reason\":\"length\"}]}\n\n",
                "data: {\"choices\":[],\"usage\":{\"completion_tokens\":4096}}\n\n",
                "data: [DONE]\n\n"
            ).as_bytes(),
            &mut |_, _| Ok(()),
        )?;
        assert_eq!(capped.finish()?, Completion::TokenLimit);
        assert_eq!(capped.text, "Partial answer");
        for reason in [
            "null",
            "\"tool_calls\"",
            "\"content_filter\"",
            "\"unknown\"",
        ] {
            let mut sse = Sse::default();
            let events = format!(
                "data: {{\"choices\":[{{\"delta\":{{\"content\":\"partial\"}},\"finish_reason\":{reason}}}]}}\n\ndata: [DONE]\n\n"
            );
            assert!(sse.push(events.as_bytes(), &mut |_, _| Ok(())).is_err());
        }
        for invalid in [
            "data: {\"choices\":[{\"delta\":{\"content\":\"unexpected\"}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
            "event: error\ndata: {\"message\":\"backend failed\"}\n\n",
        ] {
            let mut sse = Sse::default();
            sse.push(
                b"data: {\"choices\":[{\"delta\":{\"content\":\"answer\"},\"finish_reason\":\"stop\"}]}\n\n",
                &mut |_, _| Ok(()),
            )?;
            assert!(sse.push(invalid.as_bytes(), &mut |_, _| Ok(())).is_err());
        }
        let mut missing_done = Sse::default();
        missing_done.push(
            b"data: {\"choices\":[{\"delta\":{\"content\":\"answer\"},\"finish_reason\":\"stop\"}]}\n\n",
            &mut |_, _| Ok(()),
        )?;
        assert!(missing_done.finish().is_err());
        Ok(())
    }

    #[test]
    fn event_stream_media_type_accepts_utf8_parameters_and_rejects_ambiguity() {
        for media in [
            "text/event-stream",
            "TEXT/EVENT-STREAM",
            "Text/Event-Stream; CHARSET=UTF-8",
            "text/event-stream ; charset = \"utf-8\"",
        ] {
            let head =
                format!("HTTP/1.1 200 OK\r\nContent-Type: {media}\r\nContent-Length: 0\r\n\r\n");
            assert!(parse_head(head.as_bytes()).is_ok());
        }
        for media in [
            "text/event-stream-extra",
            "text/event-stream; charset=latin1",
            "text/event-stream; charset=utf-8; charset=utf-8",
            "text/event-stream; charset=\"utf-8",
            "application/json",
        ] {
            let head =
                format!("HTTP/1.1 200 OK\r\nContent-Type: {media}\r\nContent-Length: 0\r\n\r\n");
            assert!(parse_head(head.as_bytes()).is_err());
        }
    }

    #[test]
    fn startup_uses_complete_lines_and_safe_memory_message() -> Result<(), Failure> {
        assert!(
            evidence_directory(b"Protected credentials and technical evidence: C:\\unfinished")?
                .is_none()
        );
        assert_eq!(
            startup_failure(b"Development backend launch failed: Memory."),
            Failure::Memory
        );
        let Failure::Message(message) =
            startup_failure(b"Development backend launch failed: Adapter.")
        else {
            return Err(Failure::Message("Missing adapter startup guidance"));
        };
        assert!(message.contains("Get-Adapter.ps1"));
        assert!(message.contains("-NoAbliterateLora"));
        Ok(())
    }

    #[test]
    fn running_memory_failure_explains_resources_without_output_budget_guidance()
    -> Result<(), Failure> {
        let Failure::Message(message) =
            super::runtime_failure(b"Owned backend stopped after launcher failure: Memory.\n")
        else {
            return Err(Failure::Message(
                "Running memory failure lost its explicit resource guidance",
            ));
        };
        let guidance = super::retry_guidance(message);
        assert!(guidance.contains("memory"));
        assert!(guidance.contains("Close other apps"));
        assert!(guidance.contains("Retry model"));
        assert!(!guidance.contains("output budget"));
        assert!(!guidance.contains("request was too long"));
        assert!(
            matches!(super::runtime_failure(b"unrelated service exit"), Failure::Message(other) if other != message)
        );
        Ok(())
    }

    #[test]
    fn memory_retry_requires_explicit_command_and_allows_shutdown() {
        let (commands, incoming) = std::sync::mpsc::channel();
        let (events, notifications) = crate::client::event_channel();
        let mut history = vec![Message {
            role: "user",
            content: "Previous conversation must not survive New conversation".into(),
            reasoning_content: None,
        }];
        assert_eq!(
            startup_failure(b"Development backend launch failed: Memory."),
            Failure::Memory
        );
        assert!(matches!(
            startup_failure(b"Development backend launch failed: Artifacts."),
            Failure::Message(_)
        ));
        assert!(commands.send(Command::NewChat).is_ok());
        assert!(commands.send(Command::Stop).is_ok());
        assert!(
            commands
                .send(Command::Send {
                    text: "Never automatically replay this message".into(),
                    max_completion_tokens: 4096,
                    extra_effort: false,
                    enable_thinking: false,
                    abliteration_scale: super::DEFAULT_ABLITERATION_SCALE,
                })
                .is_ok()
        );
        assert!(commands.send(Command::Retry).is_ok());
        assert!(wait_for_retry(
            &incoming,
            &events,
            &mut history,
            &Event::Retryable("Select Retry model.".to_owned())
        ));
        assert!(history.is_empty());
        assert!(matches!(notifications.try_recv(), Ok(Event::Retryable(_))));
        assert!(
            matches!(notifications.try_recv(), Ok(Event::Rejected { text, retry_required: true, .. })
            if text == "Never automatically replay this message")
        );
        assert!(matches!(notifications.try_recv(), Ok(Event::Retryable(_))));
        assert!(notifications.try_recv().is_err());
        assert!(commands.send(Command::Shutdown).is_ok());
        assert!(!wait_for_retry(
            &incoming,
            &events,
            &mut history,
            &Event::Retryable("Select Retry model.".to_owned())
        ));
    }

    #[test]
    fn rejected_and_stopped_turns_restore_composer_without_committing_partial_history()
    -> Result<(), Failure> {
        let (_commands, commands) = std::sync::mpsc::channel();
        let (events, incoming) = crate::client::event_channel();
        let mut history = vec![
            Message {
                role: "user",
                content: "Earlier task".into(),
                reasoning_content: None,
            },
            Message {
                role: "assistant",
                content: "Earlier completed answer".into(),
                reasoning_content: Some("Retained reasoning".into()),
            },
        ];
        let original = serialize_request(&history, 4096, false, None)?;
        for failure in [
            Failure::Rejected("Shorten the request."),
            Failure::Stopped,
            Failure::Message("Malformed response"),
        ] {
            let mut pending = history.clone();
            pending.push(Message {
                role: "user",
                content: "Unfinished task 🌱".into(),
                reasoning_content: None,
            });
            let result = finish_turn(
                &mut history,
                pending,
                "Unfinished task 🌱",
                Err(failure),
                &events,
                &commands,
            );
            assert_eq!(result.is_ok(), matches!(failure, Failure::Rejected(_)));
            assert_eq!(serialize_request(&history, 4096, false, None)?, original);
            let event = incoming.try_recv();
            if failure == Failure::Stopped {
                assert!(matches!(event, Ok(Event::Stopping(text)) if text == "Unfinished task 🌱"));
            } else {
                assert!(
                    matches!(event, Ok(Event::Rejected { text, retry_required, .. })
                    if text == "Unfinished task 🌱" && retry_required != matches!(failure, Failure::Rejected(_)))
                );
            }
            assert!(incoming.try_recv().is_err());
        }
        // A later, explicitly submitted request is appended only after framed
        // completion; the prior rejected/stopped attempt is never replayed.
        let mut pending = history.clone();
        pending.push(Message {
            role: "user",
            content: "Reviewed task".into(),
            reasoning_content: None,
        });
        finish_turn(
            &mut history,
            pending,
            "Reviewed task",
            Ok((
                Message {
                    role: "assistant",
                    content: "New complete answer".into(),
                    reasoning_content: None,
                },
                Completion::Finished,
            )),
            &events,
            &commands,
        )?;
        assert_eq!(history.len(), 4);
        assert_eq!(history[2].content, "Reviewed task");
        assert_eq!(
            history[1].reasoning_content.as_deref(),
            Some("Retained reasoning")
        );
        assert!(
            matches!(incoming.try_recv(), Ok(Event::Committed(committed)) if committed.len() == 4 && committed[1].reasoning_content.as_deref() == Some("Retained reasoning"))
        );
        assert!(matches!(
            incoming.try_recv(),
            Ok(Event::Complete(Completion::Finished))
        ));
        Ok(())
    }

    #[test]
    fn request_limits_are_recoverable_and_output_budget_is_bounded() -> Result<(), Failure> {
        let history = [Message {
            role: "user",
            content: "Task".into(),
            reasoning_content: None,
        }];
        for budget in [1, 512, 1024, 2048, 4096] {
            let body = serialize_request(&history, budget, false, None)?;
            let json: serde_json::Value =
                serde_json::from_slice(&body).map_err(|_| Failure::Message("test request JSON"))?;
            assert_eq!(json["max_completion_tokens"], budget);
        }
        for budget in [0, 4097, u32::MAX] {
            assert!(matches!(
                serialize_request(&history, budget, false, None),
                Err(Failure::Rejected(_))
            ));
        }
        assert!(matches!(
            serialize_request(&vec![history[0].clone(); 129], 4096, false, None),
            Err(Failure::Rejected(_))
        ));
        let oversized = [Message {
            role: "user",
            content: "x".repeat(super::MAX_REQUEST),
            reasoning_content: None,
        }];
        assert!(matches!(
            serialize_request(&oversized, 4096, false, None),
            Err(Failure::Rejected(_))
        ));
        Ok(())
    }
}
