//! UI-thread microphone permission, capture and cancellation.
//!
//! No microphone API is activated until an explicit start. `WinRT` operations are
//! polled by the native timer; completed clips remain in memory and recording
//! pauses while the single local transcription request is outstanding.

use std::{
    io,
    marker::PhantomData,
    rc::Rc,
    time::{Duration, Instant},
};
use windows::{
    Media::{
        Capture::{MediaCapture, MediaCaptureInitializationSettings, StreamingCaptureMode},
        MediaProperties::{AudioEncodingProperties, AudioEncodingQuality, MediaEncodingProfile},
    },
    Security::Authorization::AppCapabilityAccess::{AppCapability, AppCapabilityAccessStatus},
    Storage::Streams::{DataReader, InMemoryRandomAccessStream},
    Win32::System::WinRT::{RO_INIT_SINGLETHREADED, RoInitialize, RoUninitialize},
    core::HSTRING,
};
use windows_future::{AsyncStatus, IAsyncAction, IAsyncOperation};

const CLIP_LIMIT: Duration = Duration::from_secs(15);
const OPERATION_LIMIT: Duration = Duration::from_secs(45);
const PERMISSION_LIMIT: Duration = Duration::from_secs(120);
const MAX_WAV: u64 = 600 * 1024;

pub(super) enum Event {
    Status(&'static str),
    Clip { generation: u64, wav: Vec<u8> },
    Failed(&'static str),
}

struct Apartment(PhantomData<Rc<()>>);
impl Apartment {
    fn initialize() -> io::Result<Self> {
        // SAFETY: This guard is created on the native UI thread before microphone
        // activation and cannot cross threads. Successful initialization is balanced.
        unsafe { RoInitialize(RO_INIT_SINGLETHREADED) }.map_err(|_| {
            io::Error::other("The UI thread cannot initialize Windows microphone access.")
        })?;
        Ok(Self(PhantomData))
    }
}
impl Drop for Apartment {
    fn drop(&mut self) {
        // SAFETY: All capture objects have been closed on this same thread before
        // this guard releases its successful RoInitialize call.
        unsafe { RoUninitialize() };
    }
}

#[derive(Default)]
struct Lifecycle {
    generation: u64,
    requested: bool,
    waiting: bool,
}
impl Lifecycle {
    fn begin(&mut self) -> io::Result<u64> {
        self.generation = self.generation.checked_add(1).ok_or_else(|| {
            io::Error::other("Microphone session limit reached. Restart the demo.")
        })?;
        self.requested = true;
        self.waiting = false;
        Ok(self.generation)
    }

    const fn stop(&mut self) {
        self.requested = false;
        self.waiting = false;
    }

    const fn matches(&self, generation: u64) -> bool {
        self.requested && self.generation == generation
    }

    const fn accept_clip(&mut self) {
        self.waiting = true;
    }

    const fn resume(&mut self, generation: u64) -> bool {
        if !self.matches(generation) || !self.waiting {
            return false;
        }
        self.waiting = false;
        true
    }
}

enum Pending {
    Permission(IAsyncOperation<AppCapabilityAccessStatus>),
    Initialize(IAsyncAction),
    Start(IAsyncAction),
    Stop(IAsyncAction),
    Read {
        operation: IAsyncOperation<u32>,
        reader: DataReader,
        size: u32,
    },
}
impl Pending {
    fn status(&self) -> windows::core::Result<AsyncStatus> {
        match self {
            Self::Permission(operation) => operation.Status(),
            Self::Initialize(operation) | Self::Start(operation) | Self::Stop(operation) => {
                operation.Status()
            }
            Self::Read { operation, .. } => operation.Status(),
        }
    }

    fn cancel(&self) {
        let _result = match self {
            Self::Permission(operation) => operation.Cancel(),
            Self::Initialize(operation) | Self::Start(operation) | Self::Stop(operation) => {
                operation.Cancel()
            }
            Self::Read { operation, .. } => operation.Cancel(),
        };
    }
}

pub(super) struct Microphone {
    lifecycle: Lifecycle,
    capability: Option<AppCapability>,
    capture: Option<MediaCapture>,
    stream: Option<InMemoryRandomAccessStream>,
    pending: Option<Pending>,
    pending_since: Instant,
    recording_since: Option<Instant>,
    checked_access: Instant,
    _apartment: Apartment,
}

impl Microphone {
    pub(super) fn new() -> io::Result<Self> {
        Ok(Self {
            lifecycle: Lifecycle::default(),
            capability: None,
            capture: None,
            stream: None,
            pending: None,
            pending_since: Instant::now(),
            recording_since: None,
            checked_access: Instant::now(),
            _apartment: Apartment::initialize()?,
        })
    }

    pub(super) fn start(&mut self) -> io::Result<u64> {
        self.stop();
        let generation = self.lifecycle.begin()?;
        let result = self.request_access();
        if result.is_err() {
            self.stop();
            return Err(io::Error::other(
                "Windows microphone permission could not be requested. Check microphone privacy settings and app capabilities.",
            ));
        }
        Ok(generation)
    }

    fn request_access(&mut self) -> windows::core::Result<()> {
        let capability = AppCapability::Create(&HSTRING::from("microphone"))?;
        let request = capability.RequestAccessAsync()?;
        self.capability = Some(capability);
        self.set_pending(Pending::Permission(request));
        Ok(())
    }

    pub(super) fn stop(&mut self) {
        self.lifecycle.stop();
        self.close_capture();
        self.capability = None;
    }

    fn close_capture(&mut self) {
        if let Some(pending) = self.pending.take() {
            pending.cancel();
        }
        self.recording_since = None;
        if let Some(capture) = self.capture.take() {
            let _result = capture.Close();
        }
        if let Some(stream) = self.stream.take() {
            let _result = stream.Close();
        }
    }

    pub(super) fn resume(&mut self, generation: u64) -> io::Result<()> {
        if !self.lifecycle.resume(generation) {
            return Ok(());
        }
        if self
            .check_access()
            .and_then(|()| self.initialize_capture())
            .is_err()
        {
            self.stop();
            return Err(io::Error::other(
                "Microphone access or capture became unavailable. Enable it again to retry.",
            ));
        }
        Ok(())
    }

    fn set_pending(&mut self, pending: Pending) {
        self.pending = Some(pending);
        self.pending_since = Instant::now();
    }

    fn check_access(&self) -> windows::core::Result<()> {
        let capability = self
            .capability
            .as_ref()
            .ok_or_else(windows::core::Error::from_thread)?;
        if capability.CheckAccess()? != AppCapabilityAccessStatus::Allowed {
            return Err(windows::core::Error::from_thread());
        }
        Ok(())
    }

    fn initialize_capture(&mut self) -> windows::core::Result<()> {
        self.check_access()?;
        let settings = MediaCaptureInitializationSettings::new()?;
        settings.SetStreamingCaptureMode(StreamingCaptureMode::Audio)?;
        let capture = MediaCapture::new()?;
        let operation = capture.InitializeWithSettingsAsync(&settings)?;
        self.capture = Some(capture);
        self.set_pending(Pending::Initialize(operation));
        self.checked_access = Instant::now();
        Ok(())
    }

    fn begin_clip(&mut self) -> windows::core::Result<()> {
        self.check_access()?;
        let profile = MediaEncodingProfile::CreateWav(AudioEncodingQuality::Low)?;
        profile.SetAudio(&AudioEncodingProperties::CreatePcm(16_000, 1, 16)?)?;
        let stream = InMemoryRandomAccessStream::new()?;
        let capture = self
            .capture
            .as_ref()
            .ok_or_else(windows::core::Error::from_thread)?;
        let operation = capture.StartRecordToStreamAsync(&profile, &stream)?;
        self.stream = Some(stream);
        self.recording_since = Some(Instant::now());
        self.set_pending(Pending::Start(operation));
        Ok(())
    }

    pub(super) fn poll(&mut self) -> Vec<Event> {
        if !self.lifecycle.requested {
            return Vec::new();
        }
        match self.advance() {
            Ok(Some(event)) => vec![event],
            Ok(None) => Vec::new(),
            Err(message) => {
                self.stop();
                vec![Event::Failed(message)]
            }
        }
    }

    fn advance(&mut self) -> Result<Option<Event>, &'static str> {
        let permission_pending = matches!(self.pending, Some(Pending::Permission(_)));
        if !permission_pending && self.checked_access.elapsed() >= Duration::from_secs(1) {
            self.check_access().map_err(
                |_| "Microphone permission was revoked. Check Windows privacy settings.",
            )?;
            self.checked_access = Instant::now();
        }
        if self
            .stream
            .as_ref()
            .is_some_and(|stream| !stream.Size().is_ok_and(|size| size <= MAX_WAV))
        {
            return Err(
                "The microphone clip exceeded its memory limit. Enable the microphone again to retry.",
            );
        }
        if matches!(self.pending, Some(Pending::Start(_)))
            && self
                .recording_since
                .is_some_and(|started| started.elapsed() >= CLIP_LIMIT)
        {
            return Err("Windows could not start the microphone within the clip deadline.");
        }
        if let Some(pending) = self.pending.take() {
            let limit = if permission_pending {
                PERMISSION_LIMIT
            } else {
                OPERATION_LIMIT
            };
            if self.pending_since.elapsed() >= limit {
                pending.cancel();
                return Err(
                    "Windows microphone access timed out. Enable the microphone again to retry.",
                );
            }
            if pending
                .status()
                .map_err(|_| "Windows microphone operation failed.")?
                == AsyncStatus::Started
            {
                self.pending = Some(pending);
                return Ok(None);
            }
            return self.completed(pending);
        }
        if self
            .recording_since
            .is_some_and(|started| started.elapsed() >= CLIP_LIMIT)
        {
            let capture = self
                .capture
                .as_ref()
                .ok_or("The microphone session ended.")?;
            let operation = capture
                .StopRecordAsync()
                .map_err(|_| "Could not finalize the microphone recording.")?;
            self.recording_since = None;
            self.set_pending(Pending::Stop(operation));
            return Ok(Some(Event::Status("Finalizing microphone recording...")));
        }
        Ok(None)
    }

    fn completed(&mut self, pending: Pending) -> Result<Option<Event>, &'static str> {
        match pending {
            Pending::Permission(operation) => {
                let status = operation
                    .GetResults()
                    .map_err(|_| "Windows microphone permission was cancelled or unavailable.")?;
                if status != AppCapabilityAccessStatus::Allowed {
                    return Err(
                        "Windows microphone permission was denied. Check microphone privacy settings and app capabilities.",
                    );
                }
                self.initialize_capture()
                    .map_err(|_| "Windows could not initialize audio-only microphone capture.")?;
                Ok(Some(Event::Status("Initializing microphone...")))
            }
            Pending::Initialize(operation) => {
                operation.GetResults().map_err(|_| "Windows could not open the microphone. Check privacy settings and the audio device.")?;
                self.begin_clip()
                    .map_err(|_| "Windows could not start PCM microphone recording.")?;
                Ok(Some(Event::Status("Starting microphone recording...")))
            }
            Pending::Start(operation) => {
                operation
                    .GetResults()
                    .map_err(|_| "Windows could not start microphone recording.")?;
                Ok(Some(Event::Status(
                    "Recording microphone audio. Disable the microphone to cancel.",
                )))
            }
            Pending::Stop(operation) => {
                operation
                    .GetResults()
                    .map_err(|_| "Windows could not finalize the microphone clip.")?;
                self.load_clip()
                    .map_err(|_| "The microphone clip could not be read within its limit.")?;
                Ok(Some(Event::Status(
                    "Preparing microphone audio for local transcription...",
                )))
            }
            Pending::Read {
                operation,
                reader,
                size,
            } => {
                let loaded = operation
                    .GetResults()
                    .map_err(|_| "The microphone clip could not be read.")?;
                if loaded != size {
                    return Err("The microphone clip was incomplete.");
                }
                let mut wav = vec![
                    0;
                    usize::try_from(size)
                        .map_err(|_| "The microphone clip length was invalid.")?
                ];
                reader
                    .ReadBytes(&mut wav)
                    .map_err(|_| "The microphone clip could not be copied.")?;
                let _result = reader.DetachStream();
                let _result = reader.Close();
                self.close_capture();
                self.lifecycle.accept_clip();
                Ok(Some(Event::Clip {
                    generation: self.lifecycle.generation,
                    wav,
                }))
            }
        }
    }

    fn load_clip(&mut self) -> windows::core::Result<()> {
        let stream = self
            .stream
            .as_ref()
            .ok_or_else(windows::core::Error::from_thread)?;
        let size = stream.Size()?;
        if !(44..=MAX_WAV).contains(&size) {
            return Err(windows::core::Error::from_thread());
        }
        let size = u32::try_from(size).map_err(|_| windows::core::Error::from_thread())?;
        let reader = DataReader::CreateDataReader(&stream.GetInputStreamAt(0)?)?;
        let operation = reader.LoadAsync(size)?;
        self.set_pending(Pending::Read {
            operation,
            reader,
            size,
        });
        Ok(())
    }
}

impl Drop for Microphone {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::Lifecycle;
    use std::io;

    #[test]
    fn cancellation_prevents_late_permission_or_transcription_from_resuming() -> io::Result<()> {
        let mut lifecycle = Lifecycle::default();
        let old = lifecycle.begin()?;
        lifecycle.accept_clip();
        lifecycle.stop();
        assert!(!lifecycle.matches(old));
        assert!(!lifecycle.resume(old));
        let current = lifecycle.begin()?;
        assert_ne!(current, old);
        lifecycle.accept_clip();
        assert!(!lifecycle.resume(old));
        assert!(lifecycle.resume(current));
        assert!(!lifecycle.resume(current));
        assert!(lifecycle.matches(current));
        Ok(())
    }

    #[test]
    fn exhausted_generations_cannot_reuse_an_old_session() {
        let mut lifecycle = Lifecycle {
            generation: u64::MAX,
            requested: false,
            waiting: false,
        };
        assert!(lifecycle.begin().is_err());
        assert!(!lifecycle.requested);
    }
}
