//! Bounded PCM playback with sequential model requests and audio prefetch.

use super::{Arc, AtomicU64, JoinHandle, Ordering, Receiver, VecDeque, io, mpsc, thread, tts};
use std::{
    cell::UnsafeCell,
    sync::{
        atomic::AtomicBool,
        mpsc::{SyncSender, TrySendError},
    },
    time::Duration,
};
use windows_sys::Win32::Media::Audio::{
    HWAVEOUT, WAVEFORMATEX, WAVEHDR, waveOutClose, waveOutOpen, waveOutPrepareHeader, waveOutReset,
    waveOutUnprepareHeader, waveOutWrite,
};

const AUDIO_FAILURE: tts::Failure =
    tts::Failure::Message("Audio playback failed. Check the Windows audio output device.");
const POLL: Duration = Duration::from_millis(5);
const PACKET_BYTES: usize = 16_384;
const PACKETS: usize = 4;

pub(super) struct Pipeline {
    finished: Receiver<Result<(), tts::Failure>>,
    cancelled: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

impl Pipeline {
    pub(super) fn start(
        client: Arc<tts::Client>,
        segments: VecDeque<String>,
        voice: String,
        generation: u64,
        current: Arc<AtomicU64>,
    ) -> io::Result<Self> {
        let (complete, finished) = mpsc::channel();
        let cancelled = Arc::new(AtomicBool::new(false));
        let stop = Arc::clone(&cancelled);
        let worker = thread::Builder::new()
            .name("bonsai-tts-prefetch".to_owned())
            .spawn(move || {
                let result = produce(&client, segments, &voice, generation, &current, &stop);
                let _sent = complete.send(result);
            })?;
        Ok(Self {
            finished,
            cancelled,
            worker: Some(worker),
        })
    }

    pub(super) fn result(&self) -> Option<Result<(), tts::Failure>> {
        match self.finished.try_recv() {
            Ok(result) => Some(result),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => Some(Err(AUDIO_FAILURE)),
        }
    }
}

impl Drop for Pipeline {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let _joined = worker.join();
        }
    }
}

fn produce(
    client: &tts::Client,
    segments: VecDeque<String>,
    voice: &str,
    generation: u64,
    current: &Arc<AtomicU64>,
    stop: &Arc<AtomicBool>,
) -> Result<(), tts::Failure> {
    let (sender, incoming) = mpsc::sync_channel(PACKETS);
    let output_stop = Arc::clone(stop);
    let output_current = Arc::clone(current);
    let playback = thread::Builder::new()
        .name("bonsai-tts-pcm".to_owned())
        .spawn(move || {
            let result = consume(&incoming, generation, &output_current, &output_stop);
            if result.is_err() {
                output_stop.store(true, Ordering::Release);
            }
            result
        })
        .map_err(|_| AUDIO_FAILURE)?;
    let synthesis = (|| {
        for text in segments {
            client.synthesize_pcm(&text, voice, generation, current, stop, |pcm| {
                for packet in pcm.chunks(PACKET_BYTES) {
                    send(&sender, packet.to_vec(), generation, current, stop)?;
                }
                Ok(())
            })?;
        }
        Ok(())
    })();
    if synthesis.is_err() {
        stop.store(true, Ordering::Release);
    }
    drop(sender);
    let played = playback.join().unwrap_or(Err(AUDIO_FAILURE));
    if let Err(failure) = played
        && failure != tts::Failure::Cancelled
    {
        return Err(failure);
    }
    synthesis?;
    played
}

fn check(generation: u64, current: &AtomicU64, stop: &AtomicBool) -> Result<(), tts::Failure> {
    if stop.load(Ordering::Acquire) || current.load(Ordering::Acquire) != generation {
        Err(tts::Failure::Cancelled)
    } else {
        Ok(())
    }
}

fn send(
    sender: &SyncSender<Vec<u8>>,
    mut pcm: Vec<u8>,
    generation: u64,
    current: &AtomicU64,
    stop: &AtomicBool,
) -> Result<(), tts::Failure> {
    loop {
        check(generation, current, stop)?;
        match sender.try_send(pcm) {
            Ok(()) => return Ok(()),
            Err(TrySendError::Full(packet)) => {
                pcm = packet;
                thread::sleep(POLL);
            }
            Err(TrySendError::Disconnected(_)) => return Err(AUDIO_FAILURE),
        }
    }
}

fn consume(
    incoming: &Receiver<Vec<u8>>,
    generation: u64,
    current: &AtomicU64,
    stop: &AtomicBool,
) -> Result<(), tts::Failure> {
    let mut output = WaveOutput::open()?;
    loop {
        check(generation, current, stop)?;
        output.reap()?;
        if output.buffers.len() >= PACKETS {
            thread::sleep(POLL);
            continue;
        }
        match incoming.recv_timeout(POLL) {
            Ok(pcm) => {
                check(generation, current, stop)?;
                output.push(pcm)?;
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    while !output.buffers.is_empty() {
        check(generation, current, stop)?;
        output.reap()?;
        thread::sleep(POLL);
    }
    Ok(())
}

struct Buffer {
    // WinMM mutates this descriptor until unprepare releases it. No Rust reference
    // to the WAVEHDR itself exists while the driver owns it.
    header: UnsafeCell<WAVEHDR>,
    _pcm: Vec<u8>,
}

struct WaveOutput {
    handle: HWAVEOUT,
    // The driver retains WAVEHDR addresses. Boxing keeps them stable as this queue moves.
    buffers: VecDeque<Box<Buffer>>,
    header_bytes: u32,
}

impl WaveOutput {
    fn open() -> Result<Self, tts::Failure> {
        let format = WAVEFORMATEX {
            wFormatTag: 1,
            nChannels: 1,
            nSamplesPerSec: 24_000,
            nAvgBytesPerSec: 48_000,
            nBlockAlign: 2,
            wBitsPerSample: 16,
            cbSize: 0,
        };
        let mut handle = std::ptr::null_mut();
        // SAFETY: Writable handle output and complete fixed PCM descriptor live through the call;
        // CALLBACK_NULL registers no callback or user-data pointer.
        if unsafe { waveOutOpen(&raw mut handle, u32::MAX, &raw const format, 0, 0, 0) } != 0 {
            return Err(AUDIO_FAILURE);
        }
        Ok(Self {
            handle,
            buffers: VecDeque::new(),
            header_bytes: u32::try_from(size_of::<WAVEHDR>()).map_err(|_| AUDIO_FAILURE)?,
        })
    }

    fn push(&mut self, mut pcm: Vec<u8>) -> Result<(), tts::Failure> {
        if pcm.is_empty() || pcm.len() > PACKET_BYTES || pcm.len() % 2 != 0 {
            return Err(AUDIO_FAILURE);
        }
        let buffer = Box::new(Buffer {
            header: UnsafeCell::new(WAVEHDR {
                lpData: pcm.as_mut_ptr(),
                dwBufferLength: u32::try_from(pcm.len()).map_err(|_| AUDIO_FAILURE)?,
                ..WAVEHDR::default()
            }),
            _pcm: pcm,
        });
        // SAFETY: This exclusive boxed header and its owned PCM allocation remain stable until
        // unprepare confirms release; the descriptor size matches the native ABI.
        if unsafe { waveOutPrepareHeader(self.handle, buffer.header.get(), self.header_bytes) } != 0
        {
            return Err(AUDIO_FAILURE);
        }
        self.buffers.push_back(buffer);
        let buffer = self.buffers.back().ok_or(AUDIO_FAILURE)?;
        // SAFETY: The prepared header and PCM remain owned by buffers until the driver releases them.
        if unsafe { waveOutWrite(self.handle, buffer.header.get(), self.header_bytes) } != 0 {
            return Err(AUDIO_FAILURE);
        }
        Ok(())
    }

    fn reap(&mut self) -> Result<(), tts::Failure> {
        while let Some(buffer) = self.buffers.front() {
            // SAFETY: Only WinMM accesses a submitted header; unprepare atomically tests whether
            // the driver released it, avoiding unsynchronized Rust reads of driver-written flags.
            match unsafe {
                waveOutUnprepareHeader(self.handle, buffer.header.get(), self.header_bytes)
            } {
                0 => {
                    self.buffers.pop_front();
                }
                33 => break, // WAVERR_STILLPLAYING: retain both allocations.
                _ => return Err(AUDIO_FAILURE),
            }
        }
        Ok(())
    }
}

impl Drop for WaveOutput {
    fn drop(&mut self) {
        // SAFETY: This thread exclusively owns the live handle; reset stops playback and returns
        // queued buffers before unprepare and release. No callbacks can outlive these buffers.
        let reset = unsafe { waveOutReset(self.handle) };
        let mut released = reset == 0;
        while let Some(buffer) = self.buffers.pop_front() {
            // SAFETY: The header remains allocated while asking the driver to release it.
            if unsafe {
                waveOutUnprepareHeader(self.handle, buffer.header.get(), self.header_bytes)
            } != 0
            {
                // An abnormal driver refusal cannot justify freeing memory still owned by WinMM.
                // This bounded allocation survives to process exit instead of becoming dangling.
                let _retained = Box::leak(buffer);
                released = false;
            }
        }
        if released {
            // SAFETY: Reset succeeded and all prepared buffers have been released.
            let _closed = unsafe { waveOutClose(self.handle) };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Arc, AtomicBool, AtomicU64, Ordering, PACKETS, check, mpsc, send};
    use crate::tts::Failure;

    #[test]
    fn bounded_prefetch_cancels_while_the_audio_queue_is_full() {
        let (sender, _incoming) = mpsc::sync_channel(PACKETS);
        for _ in 0..PACKETS {
            assert!(sender.try_send(vec![0; 2]).is_ok());
        }
        let current = Arc::new(AtomicU64::new(4));
        let stop = Arc::new(AtomicBool::new(false));
        let worker_current = Arc::clone(&current);
        let worker_stop = Arc::clone(&stop);
        let worker =
            std::thread::spawn(move || send(&sender, vec![0; 2], 4, &worker_current, &worker_stop));
        current.store(5, Ordering::Release);
        assert_eq!(worker.join().ok(), Some(Err(Failure::Cancelled)));
        stop.store(true, Ordering::Release);
        assert_eq!(check(5, &current, &stop), Err(Failure::Cancelled));
    }
}
