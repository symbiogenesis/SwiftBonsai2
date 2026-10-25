//! Owned, bounded loopback client for the native demonstration window.

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
    sync::mpsc::{Receiver, RecvTimeoutError, Sender, TryRecvError},
    thread,
    time::{Duration, Instant},
};

const POLL: Duration = Duration::from_millis(100);
const IO_POLL: Duration = Duration::from_millis(250);
const DEADLINE: Duration = Duration::from_secs(600);
const MAX_HEAD: usize = 16 * 1024;
const MAX_BODY: usize = 4 * 1024 * 1024;
const MAX_REQUEST: usize = 1024 * 1024;
const MAX_EVENT: usize = 1024 * 1024;
const READ_SIZE: usize = 8192;
const EVIDENCE_PREFIX: &str = "Protected credentials and technical evidence: ";

/// Fixed development launch inputs; paths and keys have no debug representation.
#[derive(Clone)]
pub(crate) struct Config {
    pub(crate) broker: PathBuf,
    pub(crate) data: PathBuf,
    pub(crate) evidence: PathBuf,
    pub(crate) frontend_port: u16,
    pub(crate) backend_port: u16,
}

/// Commands sent from the native UI to its single background worker.
pub(crate) enum Command {
    Send(String),
    NewChat,
    Stop,
    Retry,
    Shutdown,
}

/// Text-only notifications; no credentials or raw HTTP diagnostics cross this boundary.
pub(crate) enum Event {
    Starting,
    Ready,
    Text(String),
    Reasoning(String),
    Complete,
    Failed(String),
    Retryable(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Failure {
    Shutdown,
    Stopped,
    Memory,
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
struct Message {
    role: &'static str,
    content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    reasoning_content: Option<String>,
}

#[derive(Serialize)]
struct ChatRequest<'a> {
    messages: &'a [Message],
    max_completion_tokens: u32,
    stream: bool,
}

/// Launch the owned broker, then serve one in-memory conversation until shutdown.
#[allow(
    clippy::needless_pass_by_value,
    reason = "The background thread owns its configuration and channels until the broker is stopped"
)]
pub(crate) fn run(config: Config, commands: Receiver<Command>, events: Sender<Event>) {
    for attempt in 0..u32::MAX {
        let attempt_config = Config {
            evidence: config.evidence.join(format!("attempt-{attempt}")),
            ..config.clone()
        };
        if std::fs::create_dir(&attempt_config.evidence).is_err() {
            let _notification = events.send(Event::Failed(
                "Could not create a fresh model startup directory. Restart the demo.".to_owned(),
            ));
            return;
        }
        if notify(&events, Event::Starting).is_err() {
            return;
        }
        match worker(&attempt_config, &commands, &events) {
            Ok(()) | Err(Failure::Shutdown) => return,
            Err(Failure::Memory) => {
                if notify(&events, Event::Retryable("Not enough free memory. Swift needs 16 GiB available. Free some RAM, then select Retry model.".to_owned())).is_err()
                    || !wait_for_retry(&commands)
                {
                    return;
                }
            }
            Err(failure) => {
                let message = match failure {
                    Failure::Stopped => "Generation stopped. Restart the demo to begin a fresh model session.",
                    Failure::Message(message) => message,
                    Failure::Shutdown | Failure::Memory => return,
                };
                let _notification = events.send(Event::Failed(message.to_owned()));
                return;
            }
        }
    }
}

fn wait_for_retry(commands: &Receiver<Command>) -> bool {
    loop {
        match commands.recv_timeout(POLL) {
            Ok(Command::Retry) => return true,
            Ok(Command::Shutdown | Command::Stop) | Err(RecvTimeoutError::Disconnected) => {
                return false;
            }
            Ok(Command::Send(_) | Command::NewChat) | Err(RecvTimeoutError::Timeout) => {}
        }
    }
}

fn worker(config: &Config, commands: &Receiver<Command>, events: &Sender<Event>) -> Result<()> {
    let stdout_path = config.evidence.join("broker.stdout.log");
    let stderr_path = config.evidence.join("broker.stderr.log");
    let stdout = new_log(&stdout_path)?;
    let stderr = new_log(&stderr_path)?;
    let mut process = ProcessCommand::new(&config.broker);
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
        Failure::Message(
            "Could not start the broker. Build the demo and check its executable path.",
        )
    })?);
    let key = wait_ready(&mut broker, config, commands, &stdout_path, &stderr_path)?;
    notify(events, Event::Ready)?;
    let mut history = Vec::new();
    loop {
        if broker
            .0
            .try_wait()
            .map_err(|_| Failure::Message("Could not inspect the owned broker."))?
            .is_some()
        {
            return Err(Failure::Message(
                "The model session ended. Restart the demo to continue.",
            ));
        }
        match commands.recv_timeout(POLL) {
            Ok(Command::Shutdown) | Err(RecvTimeoutError::Disconnected) => return Ok(()),
            Ok(Command::Stop) => return Err(Failure::Stopped),
            Ok(Command::NewChat) => history.clear(),
            Ok(Command::Send(text)) => {
                if text.trim().is_empty() {
                    continue;
                }
                history.push(Message {
                    role: "user",
                    content: text,
                    reasoning_content: None,
                });
                let request = serialize_request(&history)?;
                let answer = generate(config.frontend_port, &key, &request, commands, events)?;
                history.push(answer);
                notify(events, Event::Complete)?;
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
                "Could not create fresh diagnostic logs in the demo evidence directory.",
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
                .map_err(|_| Failure::Message("The demo evidence directory is unavailable."))?;
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
                "Swift did not become ready within ten minutes. Check the retained broker logs and restart the demo.",
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
            "A local demo port is unavailable. Close the previous demo instance and restart.",
        )
    } else if text.contains("Artifacts") {
        Failure::Message(
            "The pinned Swift model or CPU runtime could not be verified. Provision both with the Swift assessment lock and restart.",
        )
    } else {
        Failure::Message(
            "Swift could not start. Check the retained broker diagnostic logs, then restart the demo.",
        )
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

fn serialize_request(history: &[Message]) -> Result<Vec<u8>> {
    if history.len() > 128 {
        return Err(Failure::Message(
            "This conversation reached the 128-message limit. Restart the demo for a new conversation.",
        ));
    }
    let body = serde_json::to_vec(&ChatRequest {
        messages: history,
        max_completion_tokens: 4096,
        stream: true,
    })
    .map_err(|_| Failure::Message("Could not prepare the chat request."))?;
    if body.len() > MAX_REQUEST {
        return Err(Failure::Message(
            "This conversation exceeded the request limit. Restart the demo for a shorter conversation.",
        ));
    }
    Ok(body)
}

fn notify(events: &Sender<Event>, event: Event) -> Result<()> {
    events.send(event).map_err(|_| Failure::Shutdown)
}

fn check_commands(commands: &Receiver<Command>) -> Result<()> {
    loop {
        match commands.try_recv() {
            Ok(Command::Shutdown) | Err(TryRecvError::Disconnected) => {
                return Err(Failure::Shutdown);
            }
            Ok(Command::Stop) => return Err(Failure::Stopped),
            Ok(Command::Send(_) | Command::NewChat | Command::Retry) => {}
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
        let result = if Instant::now() >= self.until {
            Err(Failure::Message(
                "The response exceeded the ten-minute request limit. Restart the demo to continue.",
            ))
        } else {
            check_commands(self.commands)
        };
        if let Err(failure) = result {
            self.interrupted = Some(failure);
            return Err(io::Error::new(
                io::ErrorKind::ConnectionAborted,
                "Native chat request interrupted",
            ));
        }
        Ok(())
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
                    ) => {}
                result => return result,
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
                    ) => {}
                result => return result,
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
    events: &Sender<Event>,
) -> Result<Message> {
    let address = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, port));
    let stream = TcpStream::connect_timeout(&address, Duration::from_secs(1)).map_err(|_| {
        Failure::Message("Could not connect to the owned local broker. Restart the demo.")
    })?;
    stream
        .set_read_timeout(Some(IO_POLL))
        .map_err(|_| Failure::Message("Could not configure the local response timeout."))?;
    stream
        .set_write_timeout(Some(IO_POLL))
        .map_err(|_| Failure::Message("Could not configure the local request timeout."))?;
    let mut connection = Interruptible {
        stream,
        commands,
        until: Instant::now() + DEADLINE,
        interrupted: None,
    };
    let outcome = exchange(&mut connection, port, key, request, events);
    connection.interrupted.map_or(outcome, Err)
}

fn exchange(
    connection: &mut Interruptible<'_>,
    port: u16,
    key: &str,
    request: &[u8],
    events: &Sender<Event>,
) -> Result<Message> {
    let header = format!(
        "POST /v1/chat/completions HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAuthorization: Bearer {key}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        request.len()
    );
    // Keep the sending half open: the broker interprets half-close as abandonment.
    connection
        .write_all(header.as_bytes())
        .and_then(|()| connection.write_all(request))
        .map_err(|_| Failure::Message("Could not send the chat request. Restart the demo."))?;
    let mut body = HttpBody::new(connection)?;
    let mut sse = Sse::default();
    while let Some(bytes) = body.next()? {
        sse.push(&bytes, &mut |text, reasoning| {
            if !text.is_empty() {
                notify(events, Event::Text(text.to_owned()))?;
            }
            if !reasoning.is_empty() {
                notify(events, Event::Reasoning(reasoning.to_owned()))?;
            }
            Ok(())
        })?;
    }
    sse.finish()?;
    Ok(Message {
        role: "assistant",
        content: sse.text,
        reasoning_content: (!sse.reasoning.is_empty()).then_some(sse.reasoning),
    })
}

#[derive(Clone, Copy)]
enum Framing {
    Chunked,
    Length(usize),
}

struct HttpBody<R> {
    reader: BufReader<R>,
    framing: Framing,
    remaining: usize,
    total: usize,
    done: bool,
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
        let framing = parse_head(&head)?;
        Ok(Self {
            reader,
            framing,
            remaining: 0,
            total: 0,
            done: false,
        })
    }

    fn next(&mut self) -> Result<Option<Vec<u8>>> {
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
                let mut bytes = vec![0; count];
                read_exact(&mut self.reader, &mut bytes)?;
                self.total += count;
                Ok(Some(bytes))
            }
            Framing::Chunked => self.chunk(),
        }
    }

    fn chunk(&mut self) -> Result<Option<Vec<u8>>> {
        if self.remaining == 0 {
            let line = read_line(&mut self.reader, 32)?;
            if line.is_empty() || !line.iter().all(u8::is_ascii_hexdigit) {
                return Err(Failure::Message(
                    "The local response had invalid chunk framing.",
                ));
            }
            let text = std::str::from_utf8(&line)
                .map_err(|_| Failure::Message("The local chunk size was invalid."))?;
            self.remaining = usize::from_str_radix(text, 16)
                .map_err(|_| Failure::Message("The local chunk size was invalid."))?;
            if self.remaining > MAX_BODY - self.total {
                return Err(Failure::Message(
                    "The response exceeded its four-megabyte limit.",
                ));
            }
            if self.remaining == 0 {
                if !read_line(&mut self.reader, 2)?.is_empty() {
                    return Err(Failure::Message("Response trailers are unsupported."));
                }
                self.done = true;
                return Ok(None);
            }
        }
        let count = READ_SIZE.min(self.remaining);
        let mut bytes = vec![0; count];
        read_exact(&mut self.reader, &mut bytes)?;
        self.remaining -= count;
        self.total += count;
        if self.remaining == 0 {
            let mut ending = [0; 2];
            read_exact(&mut self.reader, &mut ending)?;
            if ending != *b"\r\n" {
                return Err(Failure::Message(
                    "The local response had invalid chunk endings.",
                ));
            }
        }
        Ok(Some(bytes))
    }
}

fn read_exact(reader: &mut impl Read, bytes: &mut [u8]) -> Result<()> {
    // Do not use Read::read_exact: it retries Interrupted and could hide a
    // Shutdown/Stop request from the interruptible socket until another read.
    let mut filled = 0;
    while filled < bytes.len() {
        let count = reader.read(&mut bytes[filled..]).map_err(|_| {
            Failure::Message("The response was interrupted. Restart the demo to continue.")
        })?;
        if count == 0 {
            return Err(Failure::Message(
                "The response ended before its HTTP framing completed. Restart the demo.",
            ));
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

fn read_line(reader: &mut impl Read, max: usize) -> Result<Vec<u8>> {
    let mut line = Vec::new();
    loop {
        if line.len() == max {
            return Err(Failure::Message(
                "The HTTP framing line exceeded its limit.",
            ));
        }
        line.push(read_byte(reader)?);
        if line.ends_with(b"\r\n") {
            line.truncate(line.len() - 2);
            return Ok(line);
        }
    }
}

fn parse_head(head: &[u8]) -> Result<Framing> {
    let mut headers = [httparse::EMPTY_HEADER; 64];
    let mut response = httparse::Response::new(&mut headers);
    if !matches!(response.parse(head), Ok(httparse::Status::Complete(_)))
        || response.version != Some(1)
        || response.code != Some(200)
    {
        return Err(Failure::Message(
            "The local broker rejected the request. Restart the demo, or begin a shorter conversation.",
        ));
    }
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
    if content_type != Some(b"text/event-stream".as_slice()) {
        return Err(Failure::Message(
            "The broker did not return a text event stream.",
        ));
    }
    match (length, transfer) {
        (None, Some(value)) if value.eq_ignore_ascii_case(b"chunked") => Ok(Framing::Chunked),
        (Some(value), None) if !value.is_empty() && value.iter().all(u8::is_ascii_digit) => {
            let length = std::str::from_utf8(value)
                .ok()
                .and_then(|text| text.parse::<usize>().ok())
                .filter(|length| *length <= MAX_BODY)
                .ok_or(Failure::Message(
                    "The response length is invalid or exceeded its limit.",
                ))?;
            Ok(Framing::Length(length))
        }
        _ => Err(Failure::Message(
            "The local response had unsupported or conflicting framing.",
        )),
    }
}

#[derive(Default)]
struct Sse {
    line: Vec<u8>,
    data: Vec<u8>,
    text: String,
    reasoning: String,
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
        delta: &mut impl FnMut(&str, &str) -> Result<()>,
    ) -> Result<()> {
        for byte in bytes {
            if *byte == b'\n' {
                if self.line.last() == Some(&b'\r') {
                    self.line.pop();
                }
                if self.line.is_empty() {
                    self.dispatch(delta)?;
                } else if let Some(mut data) = self.line.strip_prefix(b"data:") {
                    if let Some(rest) = data.strip_prefix(b" ") {
                        data = rest;
                    }
                    if self.data.len() + data.len() + 1 > MAX_EVENT {
                        return Err(Failure::Message("A streaming event exceeded its limit."));
                    }
                    self.data.extend_from_slice(data);
                    self.data.push(b'\n');
                }
                self.line.clear();
            } else {
                if self.line.len() == MAX_EVENT {
                    return Err(Failure::Message("A streaming line exceeded its limit."));
                }
                self.line.push(*byte);
            }
        }
        Ok(())
    }

    fn dispatch(&mut self, delta: &mut impl FnMut(&str, &str) -> Result<()>) -> Result<()> {
        if self.data.is_empty() {
            return Ok(());
        }
        self.data.pop();
        if self.data == b"[DONE]" {
            if self.done {
                return Err(Failure::Message(
                    "The response repeated its completion marker.",
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
                let text = choice.delta.content.unwrap_or_default();
                let reasoning = choice
                    .delta
                    .reasoning_content
                    .or(choice.delta.reasoning)
                    .unwrap_or_default();
                self.text.push_str(&text);
                self.reasoning.push_str(&reasoning);
                delta(&text, &reasoning)?;
            }
        }
        self.data.clear();
        Ok(())
    }

    const fn finish(&self) -> Result<()> {
        if !self.done || !self.line.is_empty() || !self.data.is_empty() {
            return Err(Failure::Message(
                "The response ended before generation completed. Restart the demo.",
            ));
        }
        if self.text.is_empty() && self.reasoning.is_empty() {
            return Err(Failure::Message(
                "The model returned no text. Restart the demo to try again.",
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{
        Command, Failure, HttpBody, Message, Sse, evidence_directory, parse_head,
        serialize_request, startup_failure, wait_for_retry,
    };
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
        while let Some(bytes) = body.next()? {
            sse.push(&bytes, &mut |content, thought| {
                text.push_str(content);
                reasoning.push_str(thought);
                Ok(())
            })?;
        }
        sse.finish()?;
        assert_eq!(text, "Rain 雨");
        assert_eq!(reasoning, "Consider 🌱");
        let request = serialize_request(&[
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
        ])?;
        let json: serde_json::Value =
            serde_json::from_slice(&request).map_err(|_| Failure::Message("test request JSON"))?;
        assert_eq!(json["messages"][1]["reasoning_content"], "Consider 🌱");
        assert!(json["messages"][0].get("reasoning_content").is_none());
        assert_eq!(json["max_completion_tokens"], 4096);
        assert_eq!(json["stream"], true);
        assert_eq!(json.as_object().map(serde_json::Map::len), Some(3));
        Ok(())
    }

    #[test]
    fn truncated_http_and_incomplete_generation_are_rejected() -> Result<(), Failure> {
        let mut body = HttpBody::new(Fragmented(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\n\r\n5\r\nabc"))?;
        assert!(body.next().is_err());
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
        Ok(())
    }

    #[test]
    fn memory_retry_requires_explicit_command_and_allows_shutdown() {
        let (commands, incoming) = std::sync::mpsc::channel();
        assert_eq!(
            startup_failure(b"Development backend launch failed: Memory."),
            Failure::Memory
        );
        assert!(matches!(
            startup_failure(b"Development backend launch failed: Artifacts."),
            Failure::Message(_)
        ));
        assert!(commands.send(Command::NewChat).is_ok());
        assert!(commands.send(Command::Retry).is_ok());
        assert!(wait_for_retry(&incoming));
        assert!(commands.send(Command::Shutdown).is_ok());
        assert!(!wait_for_retry(&incoming));
    }
}
