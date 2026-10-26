//! Thin safe-ish wrappers over FFmpeg (ADR-0004). Only what the POC needs.

use std::ffi::{CStr, CString};
use std::os::raw::c_void;

pub use ffmpeg_sys_next as sys;
use sys::{AVPacket, AVRational};

pub fn err_str(code: i32) -> String {
    let mut buf = [0i8; 256];
    unsafe {
        sys::av_strerror(code, buf.as_mut_ptr(), buf.len());
        CStr::from_ptr(buf.as_ptr()).to_string_lossy().into_owned()
    }
}

pub fn check(code: i32, what: &str) -> anyhow::Result<i32> {
    if code < 0 {
        anyhow::bail!("{what}: {} ({code})", err_str(code))
    }
    Ok(code)
}

pub fn eagain() -> i32 {
    sys::AVERROR(sys::EAGAIN)
}

pub const fn q(num: i32, den: i32) -> AVRational {
    AVRational { num, den }
}

pub fn rescale(v: i64, from: AVRational, to: AVRational) -> i64 {
    unsafe { sys::av_rescale_q(v, from, to) }
}

pub fn cstr(s: &str) -> CString {
    CString::new(s).expect("no interior NUL")
}

/// Owned AVDictionary of options.
pub struct Dict(pub *mut sys::AVDictionary);

impl Dict {
    pub fn new() -> Self {
        Dict(std::ptr::null_mut())
    }
    pub fn set(&mut self, k: &str, v: &str) {
        let (k, v) = (cstr(k), cstr(v));
        unsafe {
            sys::av_dict_set(&mut self.0, k.as_ptr(), v.as_ptr(), 0);
        }
    }
    /// Options left in the dictionary after an open call were not consumed.
    pub fn leftover(&self) -> Vec<String> {
        let mut out = Vec::new();
        unsafe {
            let mut e: *const sys::AVDictionaryEntry = std::ptr::null();
            let empty = cstr("");
            loop {
                e = sys::av_dict_get(self.0, empty.as_ptr(), e, sys::AV_DICT_IGNORE_SUFFIX as i32);
                if e.is_null() {
                    break;
                }
                out.push(CStr::from_ptr((*e).key).to_string_lossy().into_owned());
            }
        }
        out
    }
}

impl Drop for Dict {
    fn drop(&mut self) {
        unsafe { sys::av_dict_free(&mut self.0) }
    }
}

/// Encoded packet shared by reference between consumers (fan-out without copy).
pub struct Packet {
    pub pkt: *mut AVPacket,
    pub time_base: AVRational,
    pub is_video: bool,
}

unsafe impl Send for Packet {}
unsafe impl Sync for Packet {}

impl Packet {
    /// Takes a new reference to `src` (the payload buffer is ref-counted).
    pub fn new_ref(src: *const AVPacket, time_base: AVRational, is_video: bool) -> Self {
        let pkt = unsafe { sys::av_packet_clone(src) };
        assert!(!pkt.is_null(), "av_packet_clone");
        Packet { pkt, time_base, is_video }
    }
    pub fn clone_ref(&self) -> Self {
        Packet::new_ref(self.pkt, self.time_base, self.is_video)
    }
    pub fn is_key(&self) -> bool {
        unsafe { (*self.pkt).flags & sys::AV_PKT_FLAG_KEY as i32 != 0 }
    }
    pub fn pts(&self) -> i64 {
        unsafe { (*self.pkt).pts }
    }
    pub fn size(&self) -> usize {
        unsafe { (*self.pkt).size as usize }
    }
}

impl Drop for Packet {
    fn drop(&mut self) {
        unsafe { sys::av_packet_free(&mut self.pkt) }
    }
}

/// Codec parameters snapshot used to create muxer streams.
pub struct CodecPar {
    pub par: *mut sys::AVCodecParameters,
    pub time_base: AVRational,
}

unsafe impl Send for CodecPar {}
unsafe impl Sync for CodecPar {}

impl CodecPar {
    pub fn from_ctx(ctx: *const sys::AVCodecContext) -> anyhow::Result<Self> {
        unsafe {
            let par = sys::avcodec_parameters_alloc();
            check(sys::avcodec_parameters_from_context(par, ctx), "avcodec_parameters_from_context")?;
            Ok(CodecPar { par, time_base: (*ctx).time_base })
        }
    }
}

impl Drop for CodecPar {
    fn drop(&mut self) {
        unsafe { sys::avcodec_parameters_free(&mut self.par) }
    }
}

// ---- D3D11VA hwcontext structs (libavutil/hwcontext_d3d11va.h) ----------
// Declared here because the generated bindings do not include the D3D11
// header. Only the leading fields are accessed; their layout has been stable
// since FFmpeg 4.0.

#[repr(C)]
pub struct AVD3D11VADeviceContext {
    pub device: *mut c_void,
    pub device_context: *mut c_void,
    pub video_device: *mut c_void,
    pub video_context: *mut c_void,
    pub lock: Option<unsafe extern "C" fn(*mut c_void)>,
    pub unlock: Option<unsafe extern "C" fn(*mut c_void)>,
    pub lock_ctx: *mut c_void,
}

#[repr(C)]
pub struct AVD3D11VAFramesContext {
    pub texture: *mut c_void,
    pub bind_flags: u32,
    pub misc_flags: u32,
    pub texture_infos: *mut c_void,
}

pub fn version_string() -> String {
    unsafe {
        let v = sys::avcodec_version();
        format!(
            "libavcodec {}.{}.{} ({})",
            v >> 16,
            (v >> 8) & 0xff,
            v & 0xff,
            CStr::from_ptr(sys::av_version_info()).to_string_lossy()
        )
    }
}

pub fn license_string() -> String {
    unsafe { CStr::from_ptr(sys::avcodec_license()).to_string_lossy().into_owned() }
}
