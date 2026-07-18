use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

use anyhow::Result;

use bevy::prelude::*;

use cpal::{
    SampleFormat, SampleRate, StreamConfig,
    traits::{DeviceTrait, HostTrait, StreamTrait},
};

use ringbuf::{
    HeapCons, HeapProd,
    traits::{Observer, *},
};

use rubato::{FastFixedIn, PolynomialDegree, Resampler};

/// Largest fractional drift correction [`AudioResampler::set_adjust`] will
/// honour. The async resampler is built with this much relative-ratio headroom
/// so corrections can be applied on the fly without rebuilding. Kept well above
/// the controller's own clamp so it never saturates here.
const MAX_RATIO_ADJUST: f64 = 0.05;

/// Resamples interleaved stereo audio from the core's native rate to the
/// output device rate, converting the core's `i16` samples to `f32` along the
/// way.
///
/// The core hands us a variable number of frames each call, while the
/// underlying [`FastFixedIn`] resampler wants a fixed input chunk, so incoming
/// samples are deinterleaved into per-channel buffers and consumed one full
/// chunk at a time.
///
/// [`FastFixedIn`] is an *asynchronous* resampler whose ratio can be nudged via
/// [`set_resample_ratio_relative`](Resampler::set_resample_ratio_relative)
/// without rebuilding. That is what makes per-frame drift correction cheap:
/// only a genuine change of the core's *nominal* sample rate triggers a rebuild.
pub struct AudioResampler {
    inner: FastFixedIn<f32>,
    /// Deinterleaved input awaiting a full chunk, one buffer per channel.
    in_buf: [Vec<f32>; 2],
    /// Scratch output buffers, one per channel.
    out: [Vec<f32>; 2],
    chunk_size: usize,
    /// Current nominal input (core) sample rate, tracked so [`process`] can skip
    /// rebuilding when the rate is unchanged.
    from: u32,
    /// Output (device) sample rate, needed to rebuild `inner` on a rate change.
    to: u32,
}

impl AudioResampler {
    pub fn new(from: u32, to: u32) -> Result<Self> {
        let chunk_size = 1024;
        let inner = FastFixedIn::<f32>::new(
            to as f64 / from as f64,
            1.0 + MAX_RATIO_ADJUST,
            PolynomialDegree::Cubic,
            chunk_size,
            2,
        )?;
        let out_max = inner.output_frames_max();
        Ok(Self {
            inner,
            in_buf: [Vec::new(), Vec::new()],
            out: [vec![0.0; out_max], vec![0.0; out_max]],
            chunk_size,
            from,
            to,
        })
    }

    /// Nudge the output/input ratio by `adjust` (a small signed fraction, e.g.
    /// `+0.002` for +0.2%) to compensate for clock drift between the emulator
    /// and the audio device, without rebuilding the resampler.
    ///
    /// A positive `adjust` raises the effective input rate, so the resampler
    /// emits *fewer* output frames per input frame and the downstream ring
    /// buffer drains; a negative `adjust` does the reverse and lets it fill.
    /// The change is ramped over the next chunk to avoid zipper noise, and
    /// clamped to the headroom the resampler was built with.
    pub fn set_adjust(&mut self, adjust: f64) {
        let adjust = adjust.clamp(-MAX_RATIO_ADJUST, MAX_RATIO_ADJUST);
        // rel < 1 when adjust > 0: faster input -> fewer output frames.
        let rel = 1.0 / (1.0 + adjust);
        if let Err(e) = self.inner.set_resample_ratio_relative(rel, true) {
            warn!("audio ratio adjust failed: {e}");
        }
    }

    /// Feeds interleaved stereo `i16` samples captured at `from` Hz, invoking
    /// `sink` with each resampled `(left, right)` `f32` frame.
    ///
    /// If `from` differs from the rate the resampler was last built for, the
    /// resampler is rebuilt for the new ratio. Before that, whatever the old
    /// resampler still holds — the trailing partial chunk plus its internal
    /// delay — is flushed through `sink`, so the rate change neither drops nor
    /// mis-pitches already-captured audio. Calls keeping the same `from` skip
    /// the rebuild, so this is cheap to invoke every frame.
    pub fn process(
        &mut self,
        from: u32,
        samples: &[i16],
        mut sink: impl FnMut(f32, f32),
    ) -> Result<()> {
        // `from == 0` means the core hasn't reported a rate yet; keep the
        // current resampler rather than rebuilding with a bogus ratio.
        if from != 0 && from != self.from {
            // `process` always drains down to a sub-chunk remainder, so the
            // buffer holds fewer than `chunk_size` frames here. Zero-pad that
            // remainder to a full chunk and push it through the old resampler:
            // the captured frames (and the previous chunk's delayed tail) come
            // out, while the padding zeros land in the discarded next block.
            let remainder = self.in_buf[0].len();
            if remainder > 0 {
                self.in_buf[0].resize(self.chunk_size, 0.0);
                self.in_buf[1].resize(self.chunk_size, 0.0);
                let [o0, o1] = &mut self.out;
                let (_, written) = self.inner.process_into_buffer(
                    &[&self.in_buf[0][..], &self.in_buf[1][..]],
                    &mut [&mut o0[..], &mut o1[..]],
                    None,
                )?;
                for i in 0..written {
                    sink(o0[i], o1[i]);
                }
            }
            self.in_buf[0].clear();
            self.in_buf[1].clear();

            // Rebuild for the new ratio and resize the scratch output buffers.
            self.inner = FastFixedIn::<f32>::new(
                self.to as f64 / from as f64,
                1.0 + MAX_RATIO_ADJUST,
                PolynomialDegree::Cubic,
                self.chunk_size,
                2,
            )?;
            let out_max = self.inner.output_frames_max();
            self.out = [vec![0.0; out_max], vec![0.0; out_max]];
            self.from = from;
        }

        for frame in samples.chunks_exact(2) {
            self.in_buf[0].push(frame[0] as f32 / 32767.0);
            self.in_buf[1].push(frame[1] as f32 / 32767.0);
        }

        let mut consumed = 0;
        while self.in_buf[0].len() - consumed >= self.chunk_size {
            let range = consumed..consumed + self.chunk_size;
            let [o0, o1] = &mut self.out;
            let (_, written) = self.inner.process_into_buffer(
                &[&self.in_buf[0][range.clone()], &self.in_buf[1][range]],
                &mut [&mut o0[..], &mut o1[..]],
                None,
            )?;
            for i in 0..written {
                sink(o0[i], o1[i]);
            }
            consumed += self.chunk_size;
        }

        if consumed > 0 {
            self.in_buf[0].drain(..consumed);
            self.in_buf[1].drain(..consumed);
        }
        Ok(())
    }
}

/// Wrapper that makes [`cpal::Stream`] `Send + Sync`.
///
/// cpal marks `Stream` as `!Send + !Sync` (`NotSendSyncAcrossAllPlatforms`)
/// because a few backends require the handle to stay on its creating thread.
/// We never call any functions on it so should be fine.
pub struct SendStream(#[allow(dead_code)] cpal::Stream);

// SAFETY: the ALSA stream handle is safe to move and drop across threads, and
// it is never accessed after `init_audio_stream` returns it.
unsafe impl Send for SendStream {}
unsafe impl Sync for SendStream {}

pub fn init_audio_stream(
    mut consumer: HeapCons<f32>,
    errored: Arc<AtomicBool>,
) -> Result<(f32, cpal::Stream)> {
    let host = cpal::default_host();
    let device = host.default_output_device().unwrap();

    let target = SampleRate(48000);

    let supported = device
        .supported_output_configs()?
        .find(|c| {
            c.channels() == 2
                && c.sample_format() == SampleFormat::F32
                && c.min_sample_rate() <= target
        })
        .expect("no supported config");
    let sample_rate = target.min(supported.max_sample_rate());

    // We continuously adjust the resample ratio based on how full the ring
    // buffer is, so a small buffer is desirable for tight feedback. Prefer 2048
    // frames, but clamp into the device's advertised range: if the smallest
    // supported buffer is larger than 2048 we take that (the lowest supported),
    // and if the range is unknown we let the backend choose. cpal's CoreAudio
    // backend rejects out-of-range fixed sizes with `StreamConfigNotSupported`,
    // so this clamp is what keeps macOS happy.
    const PREFERRED_BUFFER: u32 = 2048;
    let buffer_size = match supported.buffer_size() {
        cpal::SupportedBufferSize::Range { min, max } => {
            cpal::BufferSize::Fixed(PREFERRED_BUFFER.clamp(*min, *max))
        }
        cpal::SupportedBufferSize::Unknown => cpal::BufferSize::Default,
    };

    let mut config: StreamConfig = supported.with_sample_rate(sample_rate).into();
    config.channels = 2;
    config.buffer_size = buffer_size;

    info!(
        "cpal cfg: rate={} channels={} buffer={:?}",
        config.sample_rate.0, config.channels, config.buffer_size
    );

    let stream = device.build_output_stream(
        &config,
        move |output: &mut [f32], _: &cpal::OutputCallbackInfo| {
            let count = consumer.pop_slice(output);
            if count == 0 {
                output.fill(0.0);
            }
        },
        move |err| {
            // Flag the fault so the main loop drops and rebuilds this stream
            // instead of letting cpal spin on a dead one. Only the first error
            // of an episode is logged: an ALSA xrun surfaces here as a recurring
            // `POLLERR` (errno -32 / -EPIPE), and printing every occurrence
            // otherwise floods stderr with tens of thousands of identical lines.
            if !errored.swap(true, Ordering::Relaxed) {
                eprintln!("audio stream error: {err}");
            }
        },
        None,
    )?;

    stream.play()?;
    Ok((config.sample_rate.0 as f32, stream))
}

/// Frames to wait before retrying output-stream init after a failure, so a
/// device that can't be opened backs off instead of retrying every frame. ~0.5s
/// at 60 FPS.
const RECOVER_DELAY_FRAMES: u32 = 30;

/// The single, process-wide audio output.
///
/// Rather than opening one ALSA stream per emulator — which on a machine with no
/// sound server means several concurrent `dmix` clients fighting over the one
/// card, and the resulting xruns/`POLLERR` flood — every active emulator mixes
/// its resampled audio into [`Self::mix`] each frame, and [`flush`](Self::flush)
/// pushes the summed result into the one output stream.
///
/// Each emulator keeps its *own* [`AudioResampler`] (its core has its own rate
/// and clock drift); this resource owns only the shared device side.
#[derive(Resource, Default)]
pub struct AudioOutput {
    /// Producer half of the SPSC ring the cpal callback drains. `None` until the
    /// stream is built (or while a faulted stream is being rebuilt).
    producer: Option<Mutex<HeapProd<f32>>>,
    /// Output device sample rate; emulators build their resamplers against it.
    pub sample_rate: f32,
    stream: Option<SendStream>,
    /// Interleaved stereo accumulator for the frame currently being assembled.
    /// Active emulators add into it; `flush` drains and clears it.
    mix: Vec<f32>,
    /// Set by the cpal error callback when the stream faults.
    errored: Arc<AtomicBool>,
    /// Countdown of frames before a failed-to-open stream is retried.
    recover_delay: u32,
    /// Once the stream has faulted we give up on audio entirely (see
    /// [`poll_fault`](Self::poll_fault)) rather than churn rebuilds.
    disabled: bool,
}

impl AudioOutput {
    /// Builds the output stream if it isn't up yet. Call this only once there is
    /// audio to play: a stream started against an empty buffer (before any
    /// emulator is producing) faults immediately on ALSA `dmix`, whereas one
    /// opened when samples are already flowing runs cleanly. On init failure it
    /// backs off `RECOVER_DELAY_FRAMES` before retrying so it can't spam.
    ///
    /// The stream is deliberately *never* torn down on a runtime fault: dropping
    /// a cpal ALSA stream joins its I/O thread, and a thread wedged spinning on
    /// `POLLERR` never returns — doing that from the main loop would hang the
    /// whole app. A fault is instead just logged once (see the error callback).
    pub fn ensure_stream(&mut self) {
        if self.stream.is_some() || self.disabled {
            return;
        }
        if self.recover_delay > 0 {
            self.recover_delay -= 1;
            return;
        }
        let (producer, consumer) = ringbuf::HeapRb::<f32>::new(4096 * 8).split();
        self.errored.store(false, Ordering::Relaxed);
        match init_audio_stream(consumer, self.errored.clone()) {
            Ok((sample_rate, stream)) => {
                self.sample_rate = sample_rate;
                self.producer = Some(Mutex::new(producer));
                self.stream = Some(SendStream(stream));
            }
            Err(e) => {
                error!("Could not init audio: {e:#}");
                self.recover_delay = RECOVER_DELAY_FRAMES;
            }
        }
    }

    /// Whether the output is up and accepting samples. False before the stream
    /// is first built and after it has been [disabled](Self::poll_fault).
    pub fn is_ready(&self) -> bool {
        self.producer.is_some()
    }

    /// Shuts audio down for good if the stream reported a fault, *without*
    /// blocking the main thread.
    ///
    /// After a `POLLERR` cpal's I/O thread busy-spins on the dead stream, pegging
    /// a core — fatal on a machine already CPU-bound on software rendering, and
    /// the reason the app appeared to hang. Dropping the stream stops that
    /// thread, but the drop *joins* it and can take a long time (or never
    /// return), so we hand the stream to a detached thread to drop and carry on
    /// silently. We deliberately do not rebuild: the underruns come from the
    /// audio thread being starved of CPU, so a fresh stream would only fault
    /// again and churn.
    pub fn poll_fault(&mut self) {
        if self.disabled || !self.errored.load(Ordering::Relaxed) {
            return;
        }
        warn!("audio stream faulted (device starved); disabling audio");
        self.disabled = true;
        self.producer = None;
        self.mix.clear();
        self.mix.shrink_to_fit();
        if let Some(stream) = self.stream.take() {
            // SAFETY note carried by `SendStream`: the handle is safe to drop
            // off-thread. This thread may block in the drop; the app does not.
            std::thread::spawn(move || drop(stream));
        }
    }

    /// Resamples one emulator's captured `i16` samples (interleaved stereo at
    /// `from` Hz) and *adds* them into the shared mix at the frame origin, so
    /// concurrently-active emulators sum together.
    pub fn mix_in(&mut self, resampler: &mut AudioResampler, from: u32, samples: &[i16]) {
        let mix = &mut self.mix;
        let mut i = 0usize;
        let res = resampler.process(from, samples, |l, r| {
            if i + 2 > mix.len() {
                mix.resize(i + 2, 0.0);
            }
            mix[i] += l;
            mix[i + 1] += r;
            i += 2;
        });
        if let Err(e) = res {
            warn!("audio resample error: {e}");
        }
    }

    /// Pushes the assembled frame into the output ring and clears the mix.
    /// Samples are clamped to `[-1, 1]` since summing several sources can
    /// overshoot. Call once per frame, after all emulators have mixed in.
    pub fn flush(&mut self) {
        if let Some(producer) = &self.producer {
            producer
                .lock()
                .unwrap()
                .push_iter(self.mix.iter().map(|s| s.clamp(-1.0, 1.0)));
        }
        self.mix.clear();
    }

    /// Current fill of the output ring, or `None` if no stream is up. Emulators
    /// feed this into their drift controllers.
    pub fn occupied_len(&self) -> Option<usize> {
        self.producer
            .as_ref()
            .map(|p| p.lock().unwrap().occupied_len())
    }
}
