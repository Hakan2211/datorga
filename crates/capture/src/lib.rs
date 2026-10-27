//! Microphone capture for session recordings and dictation.
//!
//! The cpal stream lives on its own thread (streams are not `Send` on every host). Its callback
//! only downmixes to mono and hands buffers to a writer thread, which resamples to 16 kHz
//! (what Whisper expects), converts to i16 and passes fixed-size chunks to the caller's sink.
//! Persistence and encryption are the sink's job (`thera_core::audio`).

pub mod power;
pub mod system;

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SizedSample};

pub const SAMPLE_RATE: u32 = 16_000;

#[derive(Debug, thiserror::Error)]
pub enum CaptureError {
    #[error("Kein Mikrofon gefunden")]
    NoDevice,
    #[error("Mikrofon: {0}")]
    Device(String),
    #[error("Speichern der Aufnahme fehlgeschlagen: {0}")]
    Sink(String),
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct InputDevice {
    /// Stable cpal device id, passed back to [`Recorder::start`].
    pub id: String,
    pub name: String,
    pub is_default: bool,
}

pub fn input_devices() -> Result<Vec<InputDevice>, CaptureError> {
    let host = cpal::default_host();
    let default_id = host.default_input_device().and_then(|d| d.id().ok()).map(|id| id.to_string());
    let devices = host.input_devices().map_err(dev_err)?;
    Ok(devices
        .filter_map(|d| {
            let id = d.id().ok()?.to_string();
            Some(InputDevice { is_default: Some(&id) == default_id.as_ref(), name: d.to_string(), id })
        })
        .collect())
}

/// Receives each finished chunk of mono 16 kHz PCM. An error stops the recording.
pub type Sink = Box<dyn FnMut(&[i16]) -> Result<(), String> + Send>;

struct Shared {
    /// Peak of the most recent callback buffer, f32 bits, 0..=1.
    level: AtomicU32,
    /// 16 kHz samples handed to the sink so far.
    samples: AtomicU64,
    /// 16 kHz samples captured so far, including the chunk still being filled (timer display).
    captured: AtomicU64,
    failed: AtomicBool,
    error: Mutex<Option<String>>,
}

impl Shared {
    fn fail(&self, msg: String) {
        self.failed.store(true, Ordering::SeqCst);
        self.error.lock().unwrap().get_or_insert(msg);
    }
}

pub struct Recorder {
    shared: Arc<Shared>,
    stop_tx: mpsc::Sender<()>,
    stream_thread: JoinHandle<()>,
    writer_thread: JoinHandle<Result<(), String>>,
}

impl Recorder {
    /// Opens the device (`None` = system default) and starts recording immediately.
    pub fn start(device_id: Option<&str>, chunk_samples: usize, sink: Sink) -> Result<Self, CaptureError> {
        let shared = Arc::new(Shared {
            level: AtomicU32::new(0),
            samples: AtomicU64::new(0),
            captured: AtomicU64::new(0),
            failed: AtomicBool::new(false),
            error: Mutex::new(None),
        });
        let (stop_tx, stop_rx) = mpsc::channel::<()>();
        let (ready_tx, ready_rx) = mpsc::channel::<Result<u32, CaptureError>>();
        let (pcm_tx, pcm_rx) = mpsc::channel::<Vec<f32>>();

        let device_id = device_id.map(str::to_string);
        let s = shared.clone();
        let stream_thread = std::thread::spawn(move || {
            let stream = match open_stream(device_id.as_deref(), pcm_tx, s) {
                Ok((stream, rate)) => {
                    let _ = ready_tx.send(Ok(rate));
                    stream
                }
                Err(e) => {
                    let _ = ready_tx.send(Err(e));
                    return;
                }
            };
            // Block until stop (or the Recorder is dropped); dropping the stream closes `pcm_tx`.
            let _ = stop_rx.recv();
            drop(stream);
        });

        let in_rate = ready_rx.recv().map_err(|_| CaptureError::Device("Audio-Thread beendet".into()))??;
        let s = shared.clone();
        let writer_thread = std::thread::spawn(move || write_loop(pcm_rx, in_rate, chunk_samples, sink, s));
        Ok(Self { shared, stop_tx, stream_thread, writer_thread })
    }

    pub fn level(&self) -> f32 {
        f32::from_bits(self.shared.level.load(Ordering::Relaxed))
    }

    /// Live duration, including the chunk not yet written.
    pub fn duration_ms(&self) -> u64 {
        self.shared.captured.load(Ordering::Relaxed) * 1000 / SAMPLE_RATE as u64
    }

    /// Device lost, sink failed, etc. The recording keeps what was written so far.
    pub fn error(&self) -> Option<String> {
        self.shared.error.lock().unwrap().clone()
    }

    /// Stops the device, flushes the last partial chunk and returns the total sample count.
    pub fn stop(self) -> Result<u64, CaptureError> {
        let _ = self.stop_tx.send(());
        let _ = self.stream_thread.join();
        let flushed = self.writer_thread.join().map_err(|_| CaptureError::Sink("Schreib-Thread abgestürzt".into()))?;
        flushed.map_err(CaptureError::Sink)?;
        Ok(self.shared.samples.load(Ordering::SeqCst))
    }
}

fn dev_err(e: impl std::fmt::Display) -> CaptureError {
    CaptureError::Device(e.to_string())
}

fn open_stream(
    device_id: Option<&str>,
    tx: mpsc::Sender<Vec<f32>>,
    shared: Arc<Shared>,
) -> Result<(cpal::Stream, u32), CaptureError> {
    let host = cpal::default_host();
    let device = match device_id {
        Some(id) => host.device_by_id(&id.parse().map_err(dev_err)?).ok_or(CaptureError::NoDevice)?,
        None => host.default_input_device().ok_or(CaptureError::NoDevice)?,
    };
    let supported = device.default_input_config().map_err(dev_err)?;
    let config = supported.config();
    let channels = config.channels as usize;
    let rate = config.sample_rate;
    let stream = match supported.sample_format() {
        cpal::SampleFormat::F32 => build::<f32>(&device, config, channels, tx, shared),
        cpal::SampleFormat::I16 => build::<i16>(&device, config, channels, tx, shared),
        cpal::SampleFormat::I32 => build::<i32>(&device, config, channels, tx, shared),
        cpal::SampleFormat::U16 => build::<u16>(&device, config, channels, tx, shared),
        other => return Err(CaptureError::Device(format!("Sample-Format {other} wird nicht unterstützt"))),
    }?;
    stream.play().map_err(dev_err)?;
    Ok((stream, rate))
}

fn build<T>(
    device: &cpal::Device,
    config: cpal::StreamConfig,
    channels: usize,
    tx: mpsc::Sender<Vec<f32>>,
    shared: Arc<Shared>,
) -> Result<cpal::Stream, CaptureError>
where
    T: SizedSample,
    f32: FromSample<T>,
{
    let err_shared = shared.clone();
    device
        .build_input_stream(
            config,
            move |data: &[T], _: &cpal::InputCallbackInfo| {
                let mono: Vec<f32> = data
                    .chunks(channels)
                    .map(|frame| frame.iter().map(|s| s.to_sample::<f32>()).sum::<f32>() / channels as f32)
                    .collect();
                let peak = mono.iter().fold(0f32, |m, s| m.max(s.abs())).min(1.0);
                shared.level.store(peak.to_bits(), Ordering::Relaxed);
                let _ = tx.send(mono);
            },
            move |e| {
                // Overruns are glitches, not failures; anything else ends the recording.
                if !matches!(e.kind(), cpal::ErrorKind::Xrun) {
                    err_shared.fail(e.to_string());
                }
            },
            None,
        )
        .map_err(dev_err)
}

fn write_loop(rx: mpsc::Receiver<Vec<f32>>, in_rate: u32, chunk: usize, mut sink: Sink, shared: Arc<Shared>) -> Result<(), String> {
    let mut resampler = Resampler::new(in_rate, SAMPLE_RATE);
    let mut buf: Vec<i16> = Vec::with_capacity(chunk * 2);
    let mut emit = |pcm: &[i16]| -> Result<(), String> {
        sink(pcm).inspect_err(|e| shared.fail(e.clone()))?;
        shared.samples.fetch_add(pcm.len() as u64, Ordering::SeqCst);
        Ok(())
    };
    // Ends when the stream thread drops the stream (and with it the sender).
    // A sink error returns early; the dropped receiver then makes the callback's sends no-ops.
    for block in rx {
        let before = buf.len();
        buf.extend(resampler.process(&block).into_iter().map(to_i16));
        shared.captured.fetch_add((buf.len() - before) as u64, Ordering::Relaxed);
        while buf.len() >= chunk {
            let rest = buf.split_off(chunk);
            emit(&buf)?;
            buf = rest;
        }
    }
    if !buf.is_empty() {
        emit(&buf)?;
    }
    Ok(())
}

fn to_i16(s: f32) -> i16 {
    (s.clamp(-1.0, 1.0) * i16::MAX as f32).round() as i16
}

/// Streaming resampler: boxcar low-pass (anti-aliasing when downsampling) followed by
/// linear interpolation. Plenty for speech-to-text; state carries across blocks.
pub struct Resampler {
    /// Input samples per output sample.
    step: f64,
    /// Next output position, in input samples, relative to `prev` (index 0).
    t: f64,
    prev: f32,
    window: Vec<f32>,
    wpos: usize,
    wsum: f32,
}

impl Resampler {
    pub fn new(in_rate: u32, out_rate: u32) -> Self {
        let step = in_rate as f64 / out_rate as f64;
        let width = step.round().max(1.0) as usize;
        Self { step, t: 1.0, prev: 0.0, window: vec![0.0; width], wpos: 0, wsum: 0.0 }
    }

    pub fn process(&mut self, input: &[f32]) -> Vec<f32> {
        let filtered: Vec<f32> = input
            .iter()
            .map(|&x| {
                self.wsum += x - self.window[self.wpos];
                self.window[self.wpos] = x;
                self.wpos = (self.wpos + 1) % self.window.len();
                self.wsum / self.window.len() as f32
            })
            .collect();
        let n = filtered.len();
        let at = |i: usize| if i == 0 { self.prev } else { filtered[i - 1] };
        let mut out = Vec::with_capacity((n as f64 / self.step) as usize + 1);
        while self.t < n as f64 {
            let i = self.t.floor() as usize;
            let frac = (self.t - i as f64) as f32;
            out.push(at(i) * (1.0 - frac) + at(i + 1) * frac);
            self.t += self.step;
        }
        if n > 0 {
            self.prev = filtered[n - 1];
        }
        self.t -= n as f64;
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(rate: u32, hz: f32, secs: f32) -> Vec<f32> {
        (0..(rate as f32 * secs) as usize).map(|i| (i as f32 / rate as f32 * hz * std::f32::consts::TAU).sin() * 0.5).collect()
    }

    #[test]
    fn resampler_keeps_duration_across_odd_block_sizes() {
        for in_rate in [16_000u32, 44_100, 48_000] {
            let input = sine(in_rate, 440.0, 2.0);
            let mut r = Resampler::new(in_rate, SAMPLE_RATE);
            let out: Vec<f32> = input.chunks(997).flat_map(|b| r.process(b)).collect();
            assert!((out.len() as i64 - 32_000).abs() <= 2, "{in_rate}: {}", out.len());
        }
    }

    #[test]
    fn resampler_passes_speech_band_and_damps_above_nyquist() {
        let rms = |v: &[f32]| (v.iter().map(|x| x * x).sum::<f32>() / v.len() as f32).sqrt();
        let mut r = Resampler::new(48_000, SAMPLE_RATE);
        let voice = r.process(&sine(48_000, 300.0, 1.0));
        assert!(rms(&voice[100..]) > 0.33, "300 Hz should pass: {}", rms(&voice[100..]));
        let mut r = Resampler::new(48_000, SAMPLE_RATE);
        let hiss = r.process(&sine(48_000, 15_900.0, 1.0));
        assert!(rms(&hiss[100..]) < 0.05, "near 16 kHz should be damped: {}", rms(&hiss[100..]));
    }

    #[test]
    fn write_loop_emits_fixed_chunks_and_flushes_tail() {
        let (tx, rx) = mpsc::channel();
        let got = Arc::new(Mutex::new(Vec::<usize>::new()));
        let g = got.clone();
        let shared = Arc::new(Shared { level: AtomicU32::new(0), samples: AtomicU64::new(0), captured: AtomicU64::new(0), failed: AtomicBool::new(false), error: Mutex::new(None) });
        for b in sine(16_000, 200.0, 1.3).chunks(480) {
            tx.send(b.to_vec()).unwrap();
        }
        drop(tx);
        write_loop(rx, 16_000, 8_000, Box::new(move |c| { g.lock().unwrap().push(c.len()); Ok(()) }), shared.clone()).unwrap();
        // The interpolator holds back one sample (it needs the next one), hence 4_799 not 4_800.
        assert_eq!(*got.lock().unwrap(), vec![8_000, 8_000, 4_799]);
        assert_eq!(shared.samples.load(Ordering::SeqCst), 20_799);
        assert_eq!(shared.captured.load(Ordering::SeqCst), 20_799, "timer counts the same samples");
    }
}
