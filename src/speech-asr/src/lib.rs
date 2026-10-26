//! Bounded, Python-free Canary-Qwen 2.5B inference on the native CPU backend.
//!
//! The service owns artifact verification, authentication, process limits and
//! microphone permission. This library accepts only the selected Canary family,
//! normalized mono 16 kHz samples and complete successful recognition results.
#![forbid(unsafe_code)]

use std::{
    fs::File,
    io::{self, Read},
    path::Path,
    sync::Once,
};
use transcribe_cpp::{Backend, Model, ModelOptions, RunOptions, Session, SessionOptions};

/// Required sample rate for normalized mono input.
pub const SAMPLE_RATE: usize = 16_000;
/// Longest admitted clip, in seconds.
pub const MAX_CLIP_SECONDS: usize = 30;
const MAX_SAMPLES: usize = SAMPLE_RATE * MAX_CLIP_SECONDS;
const MAX_TEXT_BYTES: usize = 32 * 1024;
const ARCHITECTURE: &str = "canary_qwen";
const VARIANT: &str = "canary-qwen-2.5b";
static LOGGING: Once = Once::new();

/// A clonable, per-request cooperative cancellation handle.
///
/// Cancellation is checked before and after inference and by the native decoder
/// between steps. Process ownership still bounds uninterruptible encoder work.
#[derive(Clone, Debug, Default)]
pub struct Cancellation(transcribe_cpp::CancelToken);

impl Cancellation {
    /// Construct a fresh request that has not been cancelled.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Stop this request; this handle must not be reused for another request.
    pub fn cancel(&self) {
        self.0.cancel();
    }

    /// Whether cancellation has been requested.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.0.is_cancelled()
    }
}

/// A loaded Canary model and one reusable recognition session.
///
/// The caller must retain its verified model-file guard while this value lives:
/// the native runtime memory-maps weights from that immutable file.
pub struct Recognizer {
    session: Session,
}

impl Recognizer {
    /// Load the selected Canary-Qwen GGUF on CPU, without downloading anything.
    ///
    /// # Errors
    /// Rejects relative paths, malformed or unexpected model headers, a different
    /// model identity/audio contract, unavailable memory, and native load faults.
    pub fn load(path: &Path) -> io::Result<Self> {
        if !path.is_absolute() {
            return Err(invalid("Use an absolute, verified Canary model path."));
        }
        let mut file = File::open(path)?;
        if !file.metadata()?.is_file() {
            return Err(invalid("The Canary model must be a regular file."));
        }
        validate_header(&mut file)?;
        LOGGING.call_once(transcribe_cpp::disable_logging);
        let model = Model::load_with(
            path,
            &ModelOptions {
                backend: Backend::Cpu,
                device: None,
                no_mmap: false,
            },
        )
        .map_err(|error| native_error(&error))?;
        let capabilities = model.capabilities().map_err(|error| native_error(&error))?;
        if model.arch() != ARCHITECTURE
            || model.variant() != VARIANT
            || capabilities.native_sample_rate != 16_000
            || !capabilities
                .languages
                .iter()
                .any(|language| language == "en")
        {
            return Err(invalid(
                "The loaded model is not the required English Canary-Qwen 2.5B model.",
            ));
        }
        let session = model
            .session_with(&SessionOptions {
                n_threads: 4,
                n_ctx: 8192,
                ..SessionOptions::default()
            })
            .map_err(|error| native_error(&error))?;
        Ok(Self { session })
    }

    /// Recognize one complete mono 16 kHz clip of at most thirty seconds.
    ///
    /// # Errors
    /// Rejects empty, oversized or nonfinite/out-of-range samples and incomplete
    /// recognition. Native diagnostics and partial transcripts are not exposed.
    pub fn transcribe(&mut self, pcm: &[f32]) -> io::Result<String> {
        self.transcribe_cancellable(pcm, &Cancellation::new())
    }

    /// Recognize a clip with an independently clonable request cancellation flag.
    ///
    /// # Errors
    /// Returns `Interrupted` for cancellation; otherwise has the same error
    /// contract as [`Self::transcribe`]. Cancelled partial text is discarded.
    pub fn transcribe_cancellable(
        &mut self,
        pcm: &[f32],
        cancellation: &Cancellation,
    ) -> io::Result<String> {
        validate_pcm(pcm)?;
        check_cancelled(cancellation)?;
        self.session.set_cancel_token(&cancellation.0);
        let result = self.session.run(
            pcm,
            &RunOptions {
                language: Some("en".to_owned()),
                ..RunOptions::default()
            },
        );
        self.session.clear_cancel_token();
        check_cancelled(cancellation)?;
        let transcript = result.map_err(|error| native_error(&error))?;
        if transcript.text.len() > MAX_TEXT_BYTES || transcript.text.contains('\0') {
            return Err(invalid(
                "Canary returned an invalid or oversized transcript.",
            ));
        }
        Ok(transcript.text)
    }
}

fn check_cancelled(cancellation: &Cancellation) -> io::Result<()> {
    if cancellation.is_cancelled() {
        return Err(io::Error::new(
            io::ErrorKind::Interrupted,
            "Canary recognition was cancelled.",
        ));
    }
    Ok(())
}

fn validate_pcm(pcm: &[f32]) -> io::Result<()> {
    if pcm.is_empty()
        || pcm.len() > MAX_SAMPLES
        || pcm
            .iter()
            .any(|sample| !sample.is_finite() || !(-1.0..=1.0).contains(sample))
    {
        return Err(invalid(
            "Canary requires bounded, finite mono 16 kHz samples normalized to [-1, 1].",
        ));
    }
    Ok(())
}

fn validate_header(reader: &mut impl Read) -> io::Result<()> {
    let mut header = [0_u8; 24];
    reader.read_exact(&mut header)?;
    if &header[..8] != b"GGUF\x03\0\0\0" {
        return Err(invalid("Canary requires the pinned GGUF version 3 model."));
    }
    // The pinned publisher's first metadata entry names the architecture. Read
    // only this bounded field before allowing the native runtime to allocate.
    if bounded_string(reader)? != "general.architecture" {
        return Err(invalid("The Canary GGUF identity field is missing."));
    }
    let mut kind = [0_u8; 4];
    reader.read_exact(&mut kind)?;
    if u32::from_le_bytes(kind) != 8 || bounded_string(reader)? != ARCHITECTURE {
        return Err(invalid(
            "The GGUF does not contain the selected Canary architecture.",
        ));
    }
    Ok(())
}

fn bounded_string(reader: &mut impl Read) -> io::Result<String> {
    let mut length = [0_u8; 8];
    reader.read_exact(&mut length)?;
    let length = usize::try_from(u64::from_le_bytes(length))
        .ok()
        .filter(|length| *length <= 128)
        .ok_or_else(|| invalid("The Canary GGUF identity is invalid."))?;
    let mut value = vec![0_u8; length];
    reader.read_exact(&mut value)?;
    String::from_utf8(value).map_err(|_| invalid("The Canary GGUF identity is not UTF-8."))
}

fn native_error(error: &transcribe_cpp::Error) -> io::Error {
    use transcribe_cpp::Error;
    match error {
        Error::Aborted { .. } => io::Error::new(
            io::ErrorKind::Interrupted,
            "Canary recognition was cancelled.",
        ),
        Error::OutOfMemory(_) => io::Error::new(
            io::ErrorKind::OutOfMemory,
            "The native Canary model exceeded available memory.",
        ),
        Error::ModelFileNotFound(_) => io::Error::new(
            io::ErrorKind::NotFound,
            "The verified Canary model is unavailable.",
        ),
        Error::OutputTruncated { .. } | Error::OutputRepetition { .. } => {
            invalid("Canary did not produce a complete transcript.")
        }
        _ => io::Error::other("The native Canary inference request failed."),
    }
}

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

#[cfg(test)]
mod tests {
    use super::{
        ARCHITECTURE, Cancellation, MAX_SAMPLES, check_cancelled, validate_header, validate_pcm,
    };
    use std::io::{self, Cursor};

    #[test]
    fn normalized_audio_rejects_nonfinite_samples_and_clip_overflow() {
        for samples in [&[][..], &[f32::NAN], &[f32::INFINITY], &[1.01], &[-1.01]] {
            assert!(validate_pcm(samples).is_err());
        }
        assert!(validate_pcm(&[-1.0, 0.0, 1.0]).is_ok());
        assert!(validate_pcm(&vec![0.0; MAX_SAMPLES]).is_ok());
        assert!(validate_pcm(&vec![0.0; MAX_SAMPLES + 1]).is_err());
    }

    #[test]
    fn cancelled_request_handles_cannot_cancel_a_fresh_request() {
        let first = Cancellation::new();
        let worker = first.clone();
        assert!(std::thread::spawn(move || worker.cancel()).join().is_ok());
        assert_eq!(
            check_cancelled(&first).err().map(|error| error.kind()),
            Some(io::ErrorKind::Interrupted)
        );
        assert!(check_cancelled(&Cancellation::new()).is_ok());
    }

    #[test]
    fn selected_canary_identity_is_checked_before_native_loading() {
        let mut header = b"GGUF\x03\0\0\0".to_vec();
        header.extend(1574_u64.to_le_bytes());
        header.extend(67_u64.to_le_bytes());
        header.extend(20_u64.to_le_bytes());
        header.extend(b"general.architecture");
        header.extend(8_u32.to_le_bytes());
        header.extend(
            u64::try_from(ARCHITECTURE.len())
                .unwrap_or_default()
                .to_le_bytes(),
        );
        header.extend(ARCHITECTURE.as_bytes());
        assert!(validate_header(&mut Cursor::new(&header)).is_ok());
        header[0] = b'X';
        assert!(validate_header(&mut Cursor::new(&header)).is_err());
        assert!(validate_header(&mut Cursor::new(b"GGUF")).is_err());
    }
}
