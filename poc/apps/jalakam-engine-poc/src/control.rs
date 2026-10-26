//! Control server: JSON lines over a named pipe (ADR-0008).
//! The pipe DACL grants access only to the owner and SYSTEM, and remote
//! clients are rejected (threat model T-03).

use std::sync::Arc;

use anyhow::Result;
use jkp_proto::{Envelope, Request, Response, PIPE_NAME};
use windows::core::{w, HSTRING};
use windows::Win32::Foundation::{CloseHandle, LocalFree, HANDLE, HLOCAL};
use windows::Win32::Security::Authorization::{ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1};
use windows::Win32::Security::{PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES};
use windows::Win32::Storage::FileSystem::{FlushFileBuffers, ReadFile, WriteFile, PIPE_ACCESS_DUPLEX};
use windows::Win32::System::Pipes::*;

use crate::engine::Engine;

struct PipeHandle(HANDLE);
unsafe impl Send for PipeHandle {}
impl Drop for PipeHandle {
    fn drop(&mut self) {
        unsafe {
            let _ = DisconnectNamedPipe(self.0);
            let _ = CloseHandle(self.0);
        }
    }
}

fn create_instance(first: bool) -> Result<PipeHandle> {
    unsafe {
        let mut sd = PSECURITY_DESCRIPTOR::default();
        // Owner rights + SYSTEM only; protected (no inherited ACEs).
        ConvertStringSecurityDescriptorToSecurityDescriptorW(w!("D:P(A;;GA;;;OW)(A;;GA;;;SY)"), SDDL_REVISION_1, &mut sd, None)?;
        let sa = SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: sd.0,
            bInheritHandle: false.into(),
        };
        let mut open_mode = PIPE_ACCESS_DUPLEX;
        if first {
            open_mode |= windows::Win32::Storage::FileSystem::FILE_FLAG_FIRST_PIPE_INSTANCE;
        }
        let h = CreateNamedPipeW(
            &HSTRING::from(PIPE_NAME),
            open_mode,
            PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
            PIPE_UNLIMITED_INSTANCES,
            64 * 1024,
            64 * 1024,
            0,
            Some(&sa),
        );
        let _ = LocalFree(Some(HLOCAL(sd.0)));
        if h.is_invalid() {
            anyhow::bail!("CTRL.PIPE_CREATE_FAILED: {}", windows::core::Error::from_thread());
        }
        Ok(PipeHandle(h))
    }
}

pub fn serve(engine: Arc<Engine>) -> Result<()> {
    let mut first = Some(create_instance(true)?);
    std::thread::Builder::new().name("ctrl-accept".into()).spawn(move || loop {
        let pipe = match first.take().map(Ok).unwrap_or_else(|| create_instance(false)) {
            Ok(p) => p,
            Err(e) => {
                crate::log!("{e:#}");
                return;
            }
        };
        let connected = unsafe { ConnectNamedPipe(pipe.0, None) };
        if connected.is_err() {
            let err = windows::core::Error::from_thread();
            // ERROR_PIPE_CONNECTED (535): client connected between create and connect.
            if err.code().0 as u32 & 0xFFFF != 535 {
                continue;
            }
        }
        let eng = engine.clone();
        let _ = std::thread::Builder::new().name("ctrl-client".into()).spawn(move || client(pipe, eng));
    })?;
    Ok(())
}

fn client(pipe: PipeHandle, engine: Arc<Engine>) {
    let mut buf = Vec::<u8>::new();
    let mut chunk = [0u8; 4096];
    loop {
        let mut n = 0u32;
        if unsafe { ReadFile(pipe.0, Some(&mut chunk), Some(&mut n), None) }.is_err() || n == 0 {
            return;
        }
        buf.extend_from_slice(&chunk[..n as usize]);
        if buf.len() > 1 << 20 {
            return; // oversized request: drop the client
        }
        while let Some(pos) = buf.iter().position(|b| *b == b'\n') {
            let line: Vec<u8> = buf.drain(..=pos).collect();
            let reply = match serde_json::from_slice::<Envelope<Request>>(&line) {
                Ok(env) => Envelope { id: env.id, body: engine.handle(env.body) },
                Err(e) => Envelope { id: 0, body: Response::Error { code: "CTRL.BAD_REQUEST".into(), message: e.to_string() } },
            };
            let mut out = serde_json::to_vec(&reply).unwrap_or_default();
            out.push(b'\n');
            let mut written = 0u32;
            if unsafe { WriteFile(pipe.0, Some(&out), Some(&mut written), None) }.is_err() {
                return;
            }
            unsafe {
                let _ = FlushFileBuffers(pipe.0);
            }
        }
    }
}
