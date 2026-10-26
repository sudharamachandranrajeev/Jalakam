//! Audio mixer clocked by QPC, plus the shared AAC encoder (ADR-0014).
//!
//! The mixer emits exactly 48 000 samples per second of master-clock time.
//! Missing input becomes silence (underrun); excess input beyond 200 ms is
//! discarded (overrun or device-clock drift). One AAC session serves every
//! output (encoder sharing).

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

use anyhow::{bail, Result};
use crossbeam_channel::Receiver;
use jkp_proto::{EncoderStats, State};

use crate::audio::{AudioBlock, CHANNELS, SAMPLE_RATE};
use crate::bus::Bus;
use crate::clock::Clock;
use crate::ff::{self, sys, CodecPar, Packet};
use crate::status::StatusCell;

pub struct MixerShared {
    pub status: StatusCell,
    pub underruns: AtomicU64,
    pub dropped_samples: AtomicU64,
    pub packets_out: AtomicU64,
    pub encode_ns: AtomicU64,
    pub frames_in: AtomicU64,
    pub errors: AtomicU64,
    /// True while a microphone is delivering; silence is not an underrun otherwise.
    pub input_active: AtomicBool,
    pub bus: Bus,
    pub codec: String,
    stop: AtomicBool,
}

pub struct Mixer {
    pub shared: Arc<MixerShared>,
    pub codecpar: Arc<CodecPar>,
    thread: Option<JoinHandle<()>>,
}

struct Enc(*mut sys::AVCodecContext);
unsafe impl Send for Enc {}
impl Drop for Enc {
    fn drop(&mut self) {
        unsafe { sys::avcodec_free_context(&mut self.0) }
    }
}

unsafe fn open_aac(bitrate_kbps: u32) -> Result<(Enc, String)> {
    for name in ["aac", "aac_mf"] {
        let n = ff::cstr(name);
        let codec = sys::avcodec_find_encoder_by_name(n.as_ptr());
        if codec.is_null() {
            continue;
        }
        let c = sys::avcodec_alloc_context3(codec);
        (*c).sample_rate = SAMPLE_RATE as i32;
        sys::av_channel_layout_default(&mut (*c).ch_layout, CHANNELS as i32);
        (*c).sample_fmt = sys::AVSampleFormat::AV_SAMPLE_FMT_FLTP;
        (*c).bit_rate = bitrate_kbps as i64 * 1000;
        (*c).time_base = ff::q(1, SAMPLE_RATE as i32);
        (*c).flags |= sys::AV_CODEC_FLAG_GLOBAL_HEADER as i32;
        let r = sys::avcodec_open2(c, codec, std::ptr::null_mut());
        if r >= 0 {
            return Ok((Enc(c), name.to_string()));
        }
        let mut c = c;
        sys::avcodec_free_context(&mut c);
        crate::log!("audio encoder {name} failed: {}", ff::err_str(r));
    }
    bail!("ENC.AUDIO_OPEN_FAILED: no AAC encoder could be opened")
}

impl Mixer {
    pub fn start(rx: Receiver<AudioBlock>, clock: Clock, bitrate_kbps: u32) -> Result<Self> {
        let (enc, codec) = unsafe { open_aac(bitrate_kbps)? };
        let codecpar = Arc::new(CodecPar::from_ctx(enc.0)?);
        let frame_size = unsafe { (*enc.0).frame_size.max(1024) } as usize;
        let shared = Arc::new(MixerShared {
            status: StatusCell::default(),
            underruns: AtomicU64::new(0),
            dropped_samples: AtomicU64::new(0),
            packets_out: AtomicU64::new(0),
            encode_ns: AtomicU64::new(0),
            frames_in: AtomicU64::new(0),
            errors: AtomicU64::new(0),
            input_active: AtomicBool::new(false),
            bus: Bus::default(),
            codec,
            stop: AtomicBool::new(false),
        });
        shared.status.ok(State::Active);
        let sh = shared.clone();
        let thread = std::thread::Builder::new()
            .name("mixer".into())
            .spawn(move || run(&sh, enc, rx, clock, frame_size))?;
        Ok(Self { shared, codecpar, thread: Some(thread) })
    }

    pub fn stop(mut self) {
        self.shared.stop.store(true, Ordering::SeqCst);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }

    pub fn stats(&self) -> EncoderStats {
        let s = &self.shared;
        let n = s.frames_in.load(Ordering::Relaxed).max(1);
        EncoderStats {
            name: "audio".into(),
            codec: s.codec.clone(),
            hardware: false,
            zero_copy: false,
            settings: format!("{SAMPLE_RATE} Hz stereo AAC-LC"),
            status: s.status.get(),
            frames_in: s.frames_in.load(Ordering::Relaxed),
            packets_out: s.packets_out.load(Ordering::Relaxed),
            dropped_queue_full: 0,
            encode_ms_avg: s.encode_ns.load(Ordering::Relaxed) as f64 / n as f64 / 1e6,
            queue_depth: 0,
            errors: s.errors.load(Ordering::Relaxed),
        }
    }
}

fn run(sh: &MixerShared, enc: Enc, rx: Receiver<AudioBlock>, clock: Clock, frame_size: usize) {
    let mut fifo: VecDeque<f32> = VecDeque::with_capacity(SAMPLE_RATE as usize);
    // Samples per channel emitted since clock start. Starts at the current
    // clock position so audio pts line up with video tick pts.
    let mut produced: u64 = clock.now_us() as u64 * SAMPLE_RATE as u64 / 1_000_000;
    let max_fifo = (SAMPLE_RATE as usize / 5) * CHANNELS; // 200 ms
    let target_fifo = (SAMPLE_RATE as usize / 50) * CHANNELS; // 20 ms
    let mut pkt = unsafe { sys::av_packet_alloc() };
    let mut frame = unsafe { sys::av_frame_alloc() };
    unsafe {
        (*frame).nb_samples = frame_size as i32;
        (*frame).format = sys::AVSampleFormat::AV_SAMPLE_FMT_FLTP as i32;
        sys::av_channel_layout_copy(&mut (*frame).ch_layout, &(*enc.0).ch_layout);
        (*frame).sample_rate = SAMPLE_RATE as i32;
        sys::av_frame_get_buffer(frame, 0);
    }
    let mut last_input = std::time::Instant::now() - Duration::from_secs(10);
    while !sh.stop.load(Ordering::SeqCst) {
        std::thread::sleep(Duration::from_millis(5));
        while let Ok(b) = rx.try_recv() {
            fifo.extend(b.samples);
            last_input = std::time::Instant::now();
        }
        let active = last_input.elapsed() < Duration::from_millis(250);
        sh.input_active.store(active, Ordering::Relaxed);
        if fifo.len() > max_fifo {
            let excess = fifo.len() - target_fifo;
            fifo.drain(..excess);
            sh.dropped_samples.fetch_add((excess / CHANNELS) as u64, Ordering::Relaxed);
        }
        let due = clock.now_us() as u64 * SAMPLE_RATE as u64 / 1_000_000;
        while produced + frame_size as u64 <= due {
            unsafe {
                sys::av_frame_make_writable(frame);
                let l = (*frame).data[0] as *mut f32;
                let r = (*frame).data[1] as *mut f32;
                let have = fifo.len() / CHANNELS;
                if have < frame_size && active {
                    sh.underruns.fetch_add(1, Ordering::Relaxed);
                }
                for i in 0..frame_size {
                    let (a, b) = if i < have {
                        (fifo.pop_front().unwrap_or(0.0), fifo.pop_front().unwrap_or(0.0))
                    } else {
                        (0.0, 0.0)
                    };
                    *l.add(i) = a;
                    *r.add(i) = b;
                }
                (*frame).pts = produced as i64;
            }
            produced += frame_size as u64;
            sh.frames_in.fetch_add(1, Ordering::Relaxed);
            let t = std::time::Instant::now();
            let res = unsafe { encode(sh, &enc, frame, pkt) };
            sh.encode_ns.fetch_add(t.elapsed().as_nanos() as u64, Ordering::Relaxed);
            if let Err(e) = res {
                sh.errors.fetch_add(1, Ordering::Relaxed);
                sh.status.set(State::Degraded, Some("ENC.AUDIO_ENCODE_FAILED"), Some(format!("{e:#}")));
            }
        }
    }
    unsafe {
        sys::av_frame_free(&mut frame);
        sys::av_packet_free(&mut pkt);
    }
    drop(enc);
    sh.status.ok(State::Idle);
}

unsafe fn encode(sh: &MixerShared, enc: &Enc, frame: *mut sys::AVFrame, pkt: *mut sys::AVPacket) -> Result<()> {
    ff::check(sys::avcodec_send_frame(enc.0, frame), "avcodec_send_frame(aac)")?;
    loop {
        let r = sys::avcodec_receive_packet(enc.0, pkt);
        if r == ff::eagain() || r == sys::AVERROR_EOF {
            return Ok(());
        }
        ff::check(r, "avcodec_receive_packet(aac)")?;
        let p = Packet::new_ref(pkt, (*enc.0).time_base, false);
        sys::av_packet_unref(pkt);
        sh.packets_out.fetch_add(1, Ordering::Relaxed);
        sh.bus.publish(&p);
    }
}
