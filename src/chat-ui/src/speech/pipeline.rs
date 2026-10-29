//! Bounded PCM playback with sequential model requests and audio prefetch.

use super::{Arc, AtomicU64, JoinHandle, Ordering, Receiver, VecDeque, io, mpsc, thread, tts};
use std::{
    cell::UnsafeCell,
    sync::{
        atomic::AtomicBool,
        mpsc::{SyncSender, TrySendError},
    },
    time::{Duration, Instant},
};
use windows_sys::Win32::Media::Audio::{
    HWAVEOUT, WAVEFORMATEX, WAVEHDR, waveOutClose, waveOutOpen, waveOutPause, waveOutPrepareHeader,
    waveOutReset, waveOutRestart, waveOutUnprepareHeader, waveOutWrite,
};

const AUDIO_FAILURE: tts::Failure =
    tts::Failure::Message("Audio playback failed. Check the Windows audio output device.");
const POLL: Duration = Duration::from_millis(5);
// Normalize callbacks into 100 ms of mono 24 kHz PCM16, so queue bounds describe
// audio duration even when HTTP/native chunks are tiny.
const PACKET_BYTES: usize = 4_800;
const PREFETCH_PACKETS: usize = 160; // 16 seconds / 768,000 bytes, outside WinMM.
const PLAYBACK_PACKETS: usize = 8;
// Only released packets enter this pool. Full queues discard excess buffers,
// keeping retained spare PCM capacity to one WinMM queue (38,400 bytes).
const RECYCLED_PACKETS: usize = PLAYBACK_PACKETS;
// Cover the native codec's 640 ms steady bursts, including after an underrun.
const PREROLL_PACKETS: usize = 7;
const CODEC_BURST: Duration = Duration::from_millis(640);
const MAX_BUFFER_BYTES: usize = 48_000 * 120;

enum Audio {
    Frame(Vec<u8>),
    SegmentEnd,
}

pub(super) struct Pipeline {
    segments: Option<SyncSender<String>>,
    finished: Receiver<Result<(), tts::Failure>>,
    cancelled: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

impl Pipeline {
    pub(super) fn start(
        client: Arc<tts::Client>,
        voice: String,
        generation: u64,
        current: Arc<AtomicU64>,
    ) -> io::Result<Self> {
        let (complete, finished) = mpsc::channel();
        let (segments, incoming) = mpsc::sync_channel(2);
        let cancelled = Arc::new(AtomicBool::new(false));
        let stop = Arc::clone(&cancelled);
        let worker = thread::Builder::new()
            .name("bonsai-tts-prefetch".to_owned())
            .spawn(move || {
                let result = produce(&client, &incoming, &voice, generation, &current, &stop);
                let _sent = complete.send(result);
            })?;
        Ok(Self {
            segments: Some(segments),
            finished,
            cancelled,
            worker: Some(worker),
        })
    }

    pub(super) fn feed(&mut self, queued: &mut VecDeque<String>, finished: bool) {
        if let Some(sender) = &self.segments {
            while let Some(text) = queued.pop_front() {
                match sender.try_send(text) {
                    Ok(()) => {}
                    Err(TrySendError::Full(text)) => {
                        queued.push_front(text);
                        break;
                    }
                    Err(TrySendError::Disconnected(_)) => break,
                }
            }
        }
        if finished && queued.is_empty() {
            self.segments = None;
        }
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
    segments: &Receiver<String>,
    voice: &str,
    generation: u64,
    current: &Arc<AtomicU64>,
    stop: &Arc<AtomicBool>,
) -> Result<(), tts::Failure> {
    let (sender, incoming) = mpsc::sync_channel(PREFETCH_PACKETS);
    let (recycle, recycled) = mpsc::sync_channel(RECYCLED_PACKETS);
    let output_stop = Arc::clone(stop);
    let output_current = Arc::clone(current);
    let playback = thread::Builder::new()
        .name("bonsai-tts-pcm".to_owned())
        .spawn(move || {
            let result = consume(
                &incoming,
                generation,
                &output_current,
                &output_stop,
                recycle,
            );
            if result.is_err() {
                output_stop.store(true, Ordering::Release);
            }
            result
        })
        .map_err(|_| AUDIO_FAILURE)?;
    let synthesis = (|| {
        let mut pending = None;
        let mut frames = Frames::reusing(recycled);
        while let Some(text) = next_segment(segments, &mut pending, generation, current, stop)? {
            client.synthesize_pcm(&text, voice, generation, current, stop, |pcm| {
                frames.push(pcm, |packet| {
                    send(&sender, Audio::Frame(packet), generation, current, stop)
                })
            })?;
            frames
                .finish(|packet| send(&sender, Audio::Frame(packet), generation, current, stop))?;
            send(&sender, Audio::SegmentEnd, generation, current, stop)?;
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

fn next_segment(
    segments: &Receiver<String>,
    pending: &mut Option<String>,
    generation: u64,
    current: &AtomicU64,
    stop: &AtomicBool,
) -> Result<Option<String>, tts::Failure> {
    let mut text = loop {
        check(generation, current, stop)?;
        if let Some(text) = pending.take() {
            break text;
        }
        match segments.recv_timeout(POLL) {
            Ok(text) => break text,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => return Ok(None),
        }
    };
    let mut characters = text.chars().count();
    while let Ok(next) = segments.try_recv() {
        check(generation, current, stop)?;
        let count = next.chars().count();
        if characters + 1 + count > super::MAX_SEGMENT {
            *pending = Some(next);
            break;
        }
        text.push(' ');
        text.push_str(&next);
        characters += 1 + count;
    }
    Ok(Some(text))
}

struct Frames {
    pending: Vec<u8>,
    recycled: Option<Receiver<Vec<u8>>>,
}

impl Default for Frames {
    fn default() -> Self {
        Self {
            pending: Vec::with_capacity(PACKET_BYTES),
            recycled: None,
        }
    }
}

impl Frames {
    fn reusing(recycled: Receiver<Vec<u8>>) -> Self {
        Self {
            recycled: Some(recycled),
            ..Self::default()
        }
    }

    fn replacement(&self) -> Vec<u8> {
        self.recycled
            .as_ref()
            .and_then(|recycled| recycled.try_recv().ok())
            .unwrap_or_else(|| Vec::with_capacity(PACKET_BYTES))
    }

    fn push(
        &mut self,
        mut pcm: &[u8],
        mut sink: impl FnMut(Vec<u8>) -> Result<(), tts::Failure>,
    ) -> Result<(), tts::Failure> {
        while !pcm.is_empty() {
            let count = pcm.len().min(PACKET_BYTES - self.pending.len());
            self.pending.extend_from_slice(&pcm[..count]);
            pcm = &pcm[count..];
            if self.pending.len() == PACKET_BYTES {
                let replacement = self.replacement();
                sink(std::mem::replace(&mut self.pending, replacement))?;
            }
        }
        Ok(())
    }

    fn finish(
        &mut self,
        sink: impl FnOnce(Vec<u8>) -> Result<(), tts::Failure>,
    ) -> Result<(), tts::Failure> {
        if self.pending.is_empty() {
            Ok(())
        } else if self.pending.len() % 2 == 0 {
            let replacement = self.replacement();
            sink(std::mem::replace(&mut self.pending, replacement))
        } else {
            Err(AUDIO_FAILURE)
        }
    }
}

fn recycle(sender: &SyncSender<Vec<u8>>, mut pcm: Vec<u8>) {
    if pcm.capacity() == PACKET_BYTES {
        pcm.clear();
        let _returned = sender.try_send(pcm);
    }
}

fn check(generation: u64, current: &AtomicU64, stop: &AtomicBool) -> Result<(), tts::Failure> {
    if stop.load(Ordering::Acquire) || current.load(Ordering::Acquire) != generation {
        Err(tts::Failure::Cancelled)
    } else {
        Ok(())
    }
}

fn send(
    sender: &SyncSender<Audio>,
    mut pcm: Audio,
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
    incoming: &Receiver<Audio>,
    generation: u64,
    current: &AtomicU64,
    stop: &AtomicBool,
    recycle: SyncSender<Vec<u8>>,
) -> Result<(), tts::Failure> {
    consume_with(
        incoming,
        generation,
        current,
        stop,
        &mut WaveOutput::open(recycle)?,
    )
}

trait Output {
    fn queued(&self) -> usize;
    fn push(&mut self, pcm: Vec<u8>) -> Result<(), tts::Failure>;
    fn reap(&mut self) -> Result<(), tts::Failure>;
    fn pause(&mut self) -> Result<(), tts::Failure>;
    fn start(&mut self) -> Result<(), tts::Failure>;
}

fn consume_with(
    incoming: &Receiver<Audio>,
    generation: u64,
    current: &AtomicU64,
    stop: &AtomicBool,
    output: &mut impl Output,
) -> Result<(), tts::Failure> {
    output.pause()?;
    let mut started = false;
    let mut buffered = Buffered::default();
    let mut pending = None;
    let mut finished = false;
    loop {
        check(generation, current, stop)?;
        output.reap()?;
        if started && output.queued() == 0 && buffered.ready.is_empty() {
            output.pause()?;
            buffered.underrun();
            started = false;
        }
        while output.queued() < PLAYBACK_PACKETS {
            if let Some(pcm) = buffered.pop() {
                output.push(pcm)?;
                if !started
                    && (output.queued() >= PREROLL_PACKETS
                        || (buffered.ready.is_empty() && buffered.flush))
                {
                    output.start()?;
                    started = true;
                    buffered.flush = false;
                }
            } else {
                break;
            }
        }
        if finished && buffered.ready.is_empty() && output.queued() == 0 {
            return Ok(());
        }
        let audio = if pending.is_some() {
            pending.take()
        } else if finished {
            None
        } else {
            match incoming.recv_timeout(POLL) {
                Ok(audio) => Some(audio),
                Err(mpsc::RecvTimeoutError::Timeout) => None,
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    if buffered.segment_bytes != 0 {
                        return Err(AUDIO_FAILURE);
                    }
                    finished = true;
                    None
                }
            }
        };
        check(generation, current, stop)?;
        match audio {
            Some(Audio::Frame(pcm)) => {
                if let Some(pcm) = buffered.frame(pcm, Instant::now())? {
                    pending = Some(Audio::Frame(pcm));
                }
            }
            Some(Audio::SegmentEnd) => buffered.end()?,
            None => {}
        }
        if pending.is_some() || finished {
            thread::sleep(POLL);
        }
    }
}

// At most one 120-second clip of application staging, plus the 16-second
// producer channel and 800 ms WinMM queue. Slow synthesis waits for a validated
// segment instead of repeatedly playing short bursts separated by silence.
#[derive(Default)]
struct Buffered {
    ready: VecDeque<Vec<u8>>,
    segment: VecDeque<Vec<u8>>,
    bytes: usize,
    segment_bytes: usize,
    first: Option<(Instant, usize)>,
    released: bool,
    slow: bool,
    flush: bool,
}

impl Buffered {
    fn frame(&mut self, pcm: Vec<u8>, now: Instant) -> Result<Option<Vec<u8>>, tts::Failure> {
        if pcm.is_empty()
            || pcm.len() > PACKET_BYTES
            || pcm.len() % 2 != 0
            || self.segment_bytes + pcm.len() > MAX_BUFFER_BYTES
        {
            return Err(AUDIO_FAILURE);
        }
        if self.bytes + pcm.len() > MAX_BUFFER_BYTES {
            return Ok(Some(pcm));
        }
        let (first, initial) = *self.first.get_or_insert((now, pcm.len()));
        self.segment_bytes += pcm.len();
        self.bytes += pcm.len();
        if self.released {
            self.ready.push_back(pcm);
        } else {
            self.segment.push_back(pcm);
            if self.segment_bytes >= PREROLL_PACKETS * PACKET_BYTES {
                // Exclude cold first-audio latency. This compares later supply
                // time to the duration it produced, not to model load time.
                let duration = Duration::from_millis(
                    u64::try_from(self.segment_bytes - initial).map_err(|_| AUDIO_FAILURE)? / 48,
                );
                // One codec burst of jitter is normal even at real-time speed:
                // the native ramp groups 80/160/320/640 ms into separate callbacks.
                self.slow |= now.saturating_duration_since(first) > duration + CODEC_BURST;
                if !self.slow {
                    self.ready.append(&mut self.segment);
                    self.released = true;
                }
            }
        }
        Ok(None)
    }

    fn end(&mut self) -> Result<(), tts::Failure> {
        if self.segment_bytes == 0 {
            return Err(AUDIO_FAILURE);
        }
        self.ready.append(&mut self.segment);
        self.segment_bytes = 0;
        self.first = None;
        self.released = false;
        self.flush = true;
        Ok(())
    }

    fn pop(&mut self) -> Option<Vec<u8>> {
        let pcm = self.ready.pop_front()?;
        self.bytes -= pcm.len();
        Some(pcm)
    }

    const fn underrun(&mut self) {
        self.slow = true;
        self.released = false;
    }
}

struct Buffer {
    // WinMM mutates this descriptor until unprepare releases it. No Rust reference
    // to the WAVEHDR itself exists while the driver owns it.
    header: UnsafeCell<WAVEHDR>,
    pcm: Vec<u8>,
}

#[derive(Default)]
struct PlaybackBuffers {
    // WinMM retains these addresses. Moving the boxes never moves the headers.
    queued: VecDeque<Box<Buffer>>,
    // Only successfully unprepared, empty descriptors enter this bounded pool.
    #[allow(
        clippy::vec_box,
        reason = "Reused WinMM descriptors must keep their original stable addresses."
    )]
    spare: Vec<Box<Buffer>>,
}

impl PlaybackBuffers {
    fn take(&mut self, mut pcm: Vec<u8>) -> Result<Box<Buffer>, tts::Failure> {
        if self.queued.len() >= PLAYBACK_PACKETS
            || pcm.is_empty()
            || pcm.len() > PACKET_BYTES
            || pcm.len() % 2 != 0
        {
            return Err(AUDIO_FAILURE);
        }
        let header = WAVEHDR {
            lpData: pcm.as_mut_ptr(),
            dwBufferLength: u32::try_from(pcm.len()).map_err(|_| AUDIO_FAILURE)?,
            ..WAVEHDR::default()
        };
        let mut buffer = self.spare.pop().unwrap_or_else(|| {
            Box::new(Buffer {
                header: UnsafeCell::new(WAVEHDR::default()),
                pcm: Vec::new(),
            })
        });
        // The driver has released this descriptor (or has never seen it).
        *buffer.header.get_mut() = header;
        buffer.pcm = pcm;
        Ok(buffer)
    }

    fn reap(
        &mut self,
        mut unprepare: impl FnMut(*mut WAVEHDR) -> u32,
        returned: &SyncSender<Vec<u8>>,
    ) -> Result<(), tts::Failure> {
        while let Some(buffer) = self.queued.front() {
            match unprepare(buffer.header.get()) {
                0 => {
                    if let Some(mut buffer) = self.queued.pop_front() {
                        // Successful unprepare releases both native addresses.
                        // Remove every stale driver field and PCM pointer before reuse.
                        *buffer.header.get_mut() = WAVEHDR::default();
                        recycle(returned, std::mem::take(&mut buffer.pcm));
                        if self.spare.len() < PLAYBACK_PACKETS {
                            self.spare.push(buffer);
                        }
                    }
                }
                33 => break, // WAVERR_STILLPLAYING: retain both allocations.
                _ => return Err(AUDIO_FAILURE),
            }
        }
        Ok(())
    }

    fn drain(
        &mut self,
        mut unprepare: impl FnMut(*mut WAVEHDR) -> u32,
        mut retain: impl FnMut(Box<Buffer>),
    ) -> bool {
        let mut released = true;
        while let Some(buffer) = self.queued.pop_front() {
            if unprepare(buffer.header.get()) != 0 {
                retain(buffer);
                released = false;
            }
        }
        released
    }
}

struct WaveOutput {
    handle: HWAVEOUT,
    buffers: PlaybackBuffers,
    header_bytes: u32,
    recycle: SyncSender<Vec<u8>>,
}

impl WaveOutput {
    fn open(recycle: SyncSender<Vec<u8>>) -> Result<Self, tts::Failure> {
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
            buffers: PlaybackBuffers::default(),
            header_bytes: u32::try_from(size_of::<WAVEHDR>()).map_err(|_| AUDIO_FAILURE)?,
            recycle,
        })
    }
}

impl Output for WaveOutput {
    fn queued(&self) -> usize {
        self.buffers.queued.len()
    }

    fn pause(&mut self) -> Result<(), tts::Failure> {
        // SAFETY: This thread exclusively owns a live WinMM handle. Pausing
        // retains queued headers and PCM until restart/reset releases them.
        if unsafe { waveOutPause(self.handle) } == 0 {
            Ok(())
        } else {
            Err(AUDIO_FAILURE)
        }
    }

    fn start(&mut self) -> Result<(), tts::Failure> {
        // SAFETY: The live handle and queued buffers remain exclusively owned.
        if unsafe { waveOutRestart(self.handle) } == 0 {
            Ok(())
        } else {
            Err(AUDIO_FAILURE)
        }
    }

    fn push(&mut self, pcm: Vec<u8>) -> Result<(), tts::Failure> {
        let buffer = self.buffers.take(pcm)?;
        // SAFETY: This exclusive boxed header and its owned PCM allocation remain stable until
        // unprepare confirms release; the descriptor size matches the native ABI.
        if unsafe { waveOutPrepareHeader(self.handle, buffer.header.get(), self.header_bytes) } != 0
        {
            return Err(AUDIO_FAILURE);
        }
        self.buffers.queued.push_back(buffer);
        let buffer = self.buffers.queued.back().ok_or(AUDIO_FAILURE)?;
        // SAFETY: The prepared header and PCM remain owned by buffers until the driver releases them.
        if unsafe { waveOutWrite(self.handle, buffer.header.get(), self.header_bytes) } != 0 {
            return Err(AUDIO_FAILURE);
        }
        Ok(())
    }

    fn reap(&mut self) -> Result<(), tts::Failure> {
        self.buffers.reap(
            |header| {
                // SAFETY: The submitted header remains boxed and owned by buffers.
                // Unprepare tests release without reading driver-written flags in Rust.
                unsafe { waveOutUnprepareHeader(self.handle, header, self.header_bytes) }
            },
            &self.recycle,
        )
    }
}

impl Drop for WaveOutput {
    fn drop(&mut self) {
        // SAFETY: This thread exclusively owns the live handle; reset stops playback and returns
        // queued buffers before unprepare and release. No callbacks can outlive these buffers.
        let reset = unsafe { waveOutReset(self.handle) };
        let released = self.buffers.drain(
            |header| {
                // SAFETY: The header remains allocated while asking the driver to release it.
                unsafe { waveOutUnprepareHeader(self.handle, header, self.header_bytes) }
            },
            |buffer| {
                // An abnormal driver refusal cannot justify freeing memory still owned by WinMM.
                // This bounded allocation survives to process exit instead of becoming dangling.
                let _retained = Box::leak(buffer);
            },
        );
        if reset == 0 && released {
            // SAFETY: Reset succeeded and all prepared buffers have been released.
            let _closed = unsafe { waveOutClose(self.handle) };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        Arc, AtomicBool, AtomicU64, Audio, Buffered, Duration, Frames, Instant, MAX_BUFFER_BYTES,
        Ordering, Output, PACKET_BYTES, PLAYBACK_PACKETS, PREFETCH_PACKETS, PREROLL_PACKETS,
        VecDeque, check, consume_with, mpsc, send,
    };
    use crate::tts::Failure;

    #[test]
    fn streaming_segment_input_waits_through_gaps_then_finishes_or_cancels() {
        let (sender, receiver) = mpsc::sync_channel(2);
        let current = AtomicU64::new(4);
        let stop = AtomicBool::new(false);
        assert!(sender.send("First".to_owned()).is_ok());
        assert_eq!(
            super::next_segment(&receiver, &mut None, 4, &current, &stop),
            Ok(Some("First".to_owned()))
        );
        current.store(5, Ordering::Release);
        assert_eq!(
            super::next_segment(&receiver, &mut None, 4, &current, &stop),
            Err(Failure::Cancelled)
        );
        drop(sender);
        assert_eq!(
            super::next_segment(&receiver, &mut None, 5, &current, &stop),
            Ok(None)
        );
        let (sender, receiver) = mpsc::sync_channel(2);
        let waiting = std::thread::spawn(move || {
            super::next_segment(
                &receiver,
                &mut None,
                4,
                &AtomicU64::new(4),
                &AtomicBool::new(false),
            )
        });
        assert!(sender.send("Second".to_owned()).is_ok());
        assert_eq!(waiting.join().ok(), Some(Ok(Some("Second".to_owned()))));
    }

    #[test]
    fn bounded_prefetch_cancels_while_the_audio_queue_is_full() {
        let (sender, _incoming) = mpsc::sync_channel(PREFETCH_PACKETS);
        assert_eq!(PREFETCH_PACKETS * PACKET_BYTES, 48_000 * 16);
        for _ in 0..PREFETCH_PACKETS {
            assert!(sender.try_send(Audio::Frame(vec![0; PACKET_BYTES])).is_ok());
        }
        let current = Arc::new(AtomicU64::new(4));
        let stop = Arc::new(AtomicBool::new(false));
        let worker_current = Arc::clone(&current);
        let worker_stop = Arc::clone(&stop);
        let worker = std::thread::spawn(move || {
            send(
                &sender,
                Audio::Frame(vec![0; 2]),
                4,
                &worker_current,
                &worker_stop,
            )
        });
        current.store(5, Ordering::Release);
        assert_eq!(worker.join().ok(), Some(Err(Failure::Cancelled)));
        stop.store(true, Ordering::Release);
        assert_eq!(check(5, &current, &stop), Err(Failure::Cancelled));
    }

    #[test]
    fn queued_short_text_coalesces_without_reordering_or_exceeding_the_request_bound() {
        let (sender, receiver) = mpsc::sync_channel(4);
        assert!(sender.send("Hello.".to_owned()).is_ok());
        assert!(sender.send("Another sentence.".to_owned()).is_ok());
        assert!(sender.send("x".repeat(super::super::MAX_SEGMENT)).is_ok());
        assert!(sender.send("Last.".to_owned()).is_ok());
        drop(sender);
        let mut pending = None;
        let current = AtomicU64::new(4);
        let stop = AtomicBool::new(false);
        for expected in [
            "Hello. Another sentence.".to_owned(),
            "x".repeat(super::super::MAX_SEGMENT),
            "Last.".to_owned(),
        ] {
            assert_eq!(
                super::next_segment(&receiver, &mut pending, 4, &current, &stop),
                Ok(Some(expected))
            );
        }
        assert_eq!(
            super::next_segment(&receiver, &mut pending, 4, &current, &stop),
            Ok(None)
        );
    }

    #[test]
    fn tiny_pcm_callbacks_form_fixed_frames_and_flush_the_final_tail_exactly() -> Result<(), Failure>
    {
        let source: Vec<_> = (0_u8..=255).cycle().take(PACKET_BYTES * 3 + 42).collect();
        let mut frames = Frames::default();
        let mut packets = Vec::new();
        for tiny in source.chunks(2) {
            frames.push(tiny, |packet| {
                packets.push(packet);
                Ok(())
            })?;
            assert!(frames.pending.len() < PACKET_BYTES);
        }
        frames.finish(|packet| {
            packets.push(packet);
            Ok(())
        })?;
        assert_eq!(
            packets.iter().map(Vec::len).collect::<Vec<_>>(),
            [PACKET_BYTES, PACKET_BYTES, PACKET_BYTES, 42]
        );
        assert_eq!(packets.concat(), source);
        Ok(())
    }

    #[test]
    fn pcm_packet_allocations_reuse_only_returned_playback_storage() -> Result<(), Failure> {
        let (recycle, recycled) = mpsc::sync_channel(super::RECYCLED_PACKETS);
        let mut frames = Frames::reusing(recycled);
        let first_pointer = frames.pending.as_ptr();
        let mut packets = VecDeque::new();
        frames.push(&vec![1; PACKET_BYTES], |packet| {
            packets.push_back(packet);
            Ok(())
        })?;
        assert_ne!(frames.pending.as_ptr(), first_pointer);
        let first = packets.pop_front().ok_or(super::AUDIO_FAILURE)?;
        assert_eq!(first.as_ptr(), first_pointer);
        assert_eq!(first, vec![1; PACKET_BYTES]);
        super::recycle(&recycle, first);

        frames.push(&vec![2; PACKET_BYTES], |packet| {
            packets.push_back(packet);
            Ok(())
        })?;
        assert_eq!(frames.pending.as_ptr(), first_pointer);
        assert_eq!(frames.pending, Vec::<u8>::new());
        assert_eq!(frames.pending.capacity(), PACKET_BYTES);
        assert_eq!(packets.front(), Some(&vec![2; PACKET_BYTES]));

        frames.push(&[3; 42], |_| Err(super::AUDIO_FAILURE))?;
        frames.finish(|packet| {
            assert_eq!(packet.as_ptr(), first_pointer);
            assert_eq!(packet, [3; 42]);
            Ok(())
        })?;
        assert_eq!(frames.pending, Vec::<u8>::new());
        frames.finish(|_| Err(super::AUDIO_FAILURE))?;
        frames.push(&[4; 64], |_| Err(super::AUDIO_FAILURE))?;
        frames.finish(|packet| {
            assert_eq!(packet, [4; 64]);
            Ok(())
        })?;
        frames.push(&[5], |_| Err(super::AUDIO_FAILURE))?;
        assert_eq!(frames.finish(|_| Ok(())), Err(super::AUDIO_FAILURE));
        Ok(())
    }

    #[test]
    fn pcm_recycling_has_a_fixed_capacity_and_never_waits_for_the_producer() {
        let (sender, receiver) = mpsc::sync_channel(super::RECYCLED_PACKETS);
        for _ in 0..super::RECYCLED_PACKETS * 3 {
            super::recycle(&sender, vec![7; PACKET_BYTES]);
        }
        let returned: Vec<_> = receiver.try_iter().collect();
        assert_eq!(returned.len(), super::RECYCLED_PACKETS);
        assert!(returned.iter().all(Vec::is_empty));
        assert_eq!(
            returned.iter().map(Vec::capacity).sum::<usize>(),
            PLAYBACK_PACKETS * PACKET_BYTES
        );
        // A larger external allocation cannot expand retained spare capacity.
        super::recycle(&sender, vec![0; PACKET_BYTES * 2]);
        assert!(receiver.try_recv().is_err());
        drop(receiver);
        super::recycle(&sender, vec![0; PACKET_BYTES]);
    }

    #[test]
    fn playback_descriptors_reuse_released_allocations_and_clear_native_fields()
    -> Result<(), Failure> {
        let (sender, returned) = mpsc::sync_channel(PLAYBACK_PACKETS);
        let mut buffers = super::PlaybackBuffers::default();
        let mut addresses = Vec::new();
        for _ in 0..PLAYBACK_PACKETS {
            let mut buffer = buffers.take(vec![7; PACKET_BYTES])?;
            addresses.push(buffer.header.get());
            // Simulate fields written during preparation/playback before putting
            // this descriptor in the fake driver's queue. No audio device is used.
            let header = buffer.header.get_mut();
            header.dwBytesRecorded = u32::MAX;
            header.dwFlags = u32::MAX;
            header.dwLoops = u32::MAX;
            header.dwUser = usize::MAX;
            header.lpNext = std::ptr::dangling_mut();
            header.reserved = usize::MAX;
            buffers.queued.push_back(buffer);
        }
        for _ in 0..16 {
            buffers.reap(
                |header| {
                    assert!(addresses.contains(&header));
                    0
                },
                &sender,
            )?;
            assert!(buffers.queued.is_empty());
            assert_eq!(buffers.spare.len(), PLAYBACK_PACKETS);
            assert_eq!(returned.try_iter().count(), PLAYBACK_PACKETS);
            for invalid in [Vec::new(), vec![0], vec![0; PACKET_BYTES + 2]] {
                assert!(buffers.take(invalid).is_err());
                assert_eq!(buffers.spare.len(), PLAYBACK_PACKETS);
            }
            for buffer in &mut buffers.spare {
                assert_eq!(buffer.pcm, b"");
                assert_eq!(buffer.pcm.capacity(), 0);
                let header = buffer.header.get_mut();
                assert!(header.lpData.is_null());
                assert_eq!({ header.dwBufferLength }, 0);
                assert_eq!({ header.dwBytesRecorded }, 0);
                assert_eq!({ header.dwFlags }, 0);
                assert_eq!({ header.dwLoops }, 0);
                assert_eq!({ header.dwUser }, 0);
                assert!(header.lpNext.is_null());
                assert_eq!({ header.reserved }, 0);
            }
            for index in 0..PLAYBACK_PACKETS {
                let bytes = if index == 0 { 42 } else { PACKET_BYTES };
                let mut pcm = Vec::with_capacity(PACKET_BYTES);
                pcm.resize(bytes, 3);
                let pcm_address = pcm.as_ptr();
                let mut buffer = buffers.take(pcm)?;
                assert!(addresses.contains(&buffer.header.get()));
                let header = buffer.header.get_mut();
                assert_eq!(header.lpData.cast_const(), pcm_address);
                assert_eq!(usize::try_from(header.dwBufferLength).ok(), Some(bytes));
                assert_eq!(buffer.pcm, vec![3; bytes]);
                buffers.queued.push_back(buffer);
            }
            assert!(buffers.spare.is_empty());
            // Rejection cannot consume a spare descriptor or exceed the native
            // queue bound, even when a caller bypasses the consume loop.
            assert!(buffers.take(vec![0; 2]).is_err());
            assert_eq!(buffers.queued.len(), PLAYBACK_PACKETS);
            assert_eq!(
                buffers.queued.front().map(|buffer| buffer.pcm.len()),
                Some(42)
            );
        }
        Ok(())
    }

    #[test]
    fn playback_descriptors_keep_driver_owned_storage_on_unprepare_refusal() -> Result<(), Failure>
    {
        let (sender, returned) = mpsc::sync_channel(PLAYBACK_PACKETS);
        let mut buffers = super::PlaybackBuffers::default();
        let buffer = buffers.take(vec![9; PACKET_BYTES])?;
        let header = buffer.header.get();
        let pcm = buffer.pcm.as_ptr();
        buffers.queued.push_back(buffer);
        for result in [33, 1] {
            assert_eq!(
                buffers.reap(
                    |address| {
                        assert_eq!(address, header);
                        result
                    },
                    &sender
                ),
                if result == 33 {
                    Ok(())
                } else {
                    Err(super::AUDIO_FAILURE)
                }
            );
            assert_eq!(buffers.queued.len(), 1);
            assert!(buffers.spare.is_empty());
            assert!(returned.try_recv().is_err());
            let retained = buffers.queued.front().ok_or(super::AUDIO_FAILURE)?;
            assert_eq!(retained.header.get(), header);
            assert_eq!(retained.pcm.as_ptr(), pcm);
            assert_eq!(retained.pcm, vec![9; PACKET_BYTES]);
        }
        buffers.reap(|_| 0, &sender)?;
        assert_eq!(buffers.spare.len(), 1);
        assert_eq!(returned.try_recv().ok().map(|pcm| pcm.as_ptr()), Some(pcm));
        Ok(())
    }

    #[test]
    fn playback_descriptor_teardown_quarantines_only_refused_active_buffers() -> Result<(), Failure>
    {
        let (sender, returned) = mpsc::sync_channel(PLAYBACK_PACKETS);
        let mut buffers = super::PlaybackBuffers::default();
        // Establish one spare descriptor. Teardown must never ask the driver to
        // unprepare it again or quarantine already released storage.
        let spare = buffers.take(vec![1; PACKET_BYTES])?;
        let spare_address = spare.header.get();
        buffers.queued.push_back(spare);
        buffers.reap(|_| 0, &sender)?;
        assert!(returned.try_recv().is_ok());
        let mut active = Vec::new();
        for _ in 0..3 {
            let buffer = super::PlaybackBuffers::default().take(vec![8; PACKET_BYTES])?;
            active.push((buffer.header.get(), buffer.pcm.as_ptr()));
            buffers.queued.push_back(buffer);
        }
        let mut calls = 0;
        let mut quarantined = Vec::new();
        assert!(!buffers.drain(
            |header| {
                assert_ne!(header, spare_address);
                assert_eq!(header, active[calls].0);
                calls += 1;
                u32::from(calls != 2)
            },
            |buffer| quarantined.push(buffer)
        ));
        assert_eq!(calls, 3);
        assert!(buffers.queued.is_empty());
        assert_eq!(buffers.spare.len(), 1);
        assert_eq!(quarantined.len(), 2);
        drop(buffers);
        for (buffer, index) in quarantined.iter().zip([0, 2]) {
            assert_eq!(buffer.header.get(), active[index].0);
            assert_eq!(buffer.pcm.as_ptr(), active[index].1);
            assert_eq!(buffer.pcm, vec![8; PACKET_BYTES]);
        }
        Ok(())
    }

    #[test]
    fn fast_supply_primes_after_cold_start_but_slow_supply_waits_for_validated_segment()
    -> Result<(), Failure> {
        let cold = Instant::now() + Duration::from_secs(90);
        let mut fast = Buffered::default();
        let mut slow = Buffered::default();
        for index in 0..PREROLL_PACKETS {
            let tick = u64::try_from(index).map_err(|_| super::AUDIO_FAILURE)?;
            assert!(
                fast.frame(
                    vec![1; PACKET_BYTES],
                    cold + Duration::from_millis(tick * 50)
                )?
                .is_none()
            );
            assert!(
                slow.frame(
                    vec![2; PACKET_BYTES],
                    cold + Duration::from_millis(tick * 800)
                )?
                .is_none()
            );
            if index + 1 < PREROLL_PACKETS {
                assert!(fast.ready.is_empty());
            }
            assert!(slow.ready.is_empty());
        }
        assert_eq!(fast.ready.len(), PREROLL_PACKETS);
        assert!(!fast.slow);
        assert!(slow.slow);
        slow.end()?;
        assert_eq!(slow.ready.len(), PREROLL_PACKETS);
        assert!(slow.flush);
        Ok(())
    }

    #[test]
    fn realtime_native_callback_ramp_keeps_streaming_despite_burst_jitter() -> Result<(), Failure> {
        let now = Instant::now() + Duration::from_secs(90);
        let mut buffered = Buffered::default();
        let mut frames = Frames::default();
        for (millis, bytes) in [(80, 3_840), (240, 7_680), (560, 15_360), (1_200, 30_720)] {
            frames.push(&vec![0; bytes], |frame| {
                assert!(
                    buffered
                        .frame(frame, now + Duration::from_millis(millis))?
                        .is_none()
                );
                Ok(())
            })?;
        }
        assert!(!buffered.slow);
        assert_eq!(buffered.ready.len(), 12);
        Ok(())
    }

    #[test]
    fn underrun_requires_complete_segments_and_staging_rejects_overlong_audio()
    -> Result<(), Failure> {
        let now = Instant::now();
        let mut buffered = Buffered::default();
        buffered.underrun();
        for _ in 0..MAX_BUFFER_BYTES / PACKET_BYTES {
            assert!(buffered.frame(vec![0; PACKET_BYTES], now)?.is_none());
        }
        assert!(buffered.ready.is_empty());
        assert_eq!(buffered.bytes, MAX_BUFFER_BYTES);
        assert!(buffered.frame(vec![0; 2], now).is_err());
        buffered.end()?;
        assert_eq!(buffered.frame(vec![0; 2], now)?, Some(vec![0; 2]));
        assert!(buffered.pop().is_some());
        assert!(buffered.frame(vec![0; 2], now)?.is_none());
        assert!(buffered.slow);
        Ok(())
    }

    #[derive(Default)]
    struct RecordingOutput {
        queued: VecDeque<Vec<u8>>,
        played: Vec<u8>,
        starts: Vec<usize>,
        paused: bool,
        maximum: usize,
    }

    impl Output for RecordingOutput {
        fn queued(&self) -> usize {
            self.queued.len()
        }

        fn push(&mut self, pcm: Vec<u8>) -> Result<(), Failure> {
            self.queued.push_back(pcm);
            self.maximum = self.maximum.max(self.queued.len());
            Ok(())
        }

        fn reap(&mut self) -> Result<(), Failure> {
            if !self.paused
                && let Some(pcm) = self.queued.pop_front()
            {
                self.played.extend(pcm);
            }
            Ok(())
        }

        fn pause(&mut self) -> Result<(), Failure> {
            self.paused = true;
            Ok(())
        }

        fn start(&mut self) -> Result<(), Failure> {
            self.starts.push(self.queued.len());
            self.paused = false;
            Ok(())
        }
    }

    #[test]
    fn synthetic_output_primes_and_drains_short_final_segments_without_an_audio_device()
    -> Result<(), Failure> {
        for frames in [1, PREROLL_PACKETS + 5] {
            let (sender, incoming) = mpsc::sync_channel(PREFETCH_PACKETS);
            for _ in 0..frames {
                assert!(sender.send(Audio::Frame(vec![7; PACKET_BYTES])).is_ok());
            }
            assert!(sender.send(Audio::SegmentEnd).is_ok());
            drop(sender);
            let mut output = RecordingOutput::default();
            consume_with(
                &incoming,
                4,
                &AtomicU64::new(4),
                &AtomicBool::new(false),
                &mut output,
            )?;
            assert_eq!(output.played, vec![7; PACKET_BYTES * frames]);
            assert_eq!(output.starts.first(), Some(&frames.min(PREROLL_PACKETS)));
            assert!(output.maximum <= PLAYBACK_PACKETS);
        }
        let (sender, incoming) = mpsc::sync_channel(PREFETCH_PACKETS);
        assert!(sender.send(Audio::Frame(vec![0; 2])).is_ok());
        drop(sender);
        assert!(
            consume_with(
                &incoming,
                4,
                &AtomicU64::new(4),
                &AtomicBool::new(false),
                &mut RecordingOutput::default()
            )
            .is_err()
        );
        assert_eq!(
            consume_with(
                &incoming,
                4,
                &AtomicU64::new(5),
                &AtomicBool::new(false),
                &mut RecordingOutput::default()
            ),
            Err(Failure::Cancelled)
        );
        Ok(())
    }
}
