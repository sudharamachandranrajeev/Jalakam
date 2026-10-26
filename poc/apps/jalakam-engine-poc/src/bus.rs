//! Packet fan-out from one encoder to N outputs with bounded per-consumer
//! queues (ADR-0014). Publishing never blocks: a full queue drops the packet
//! and, for video, makes that consumer wait for the next keyframe.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

use crossbeam_channel::{Receiver, Sender, TrySendError};
use parking_lot::Mutex;

use crate::ff::Packet;

pub struct Subscriber {
    tx: Sender<Packet>,
    pub needs_key: AtomicBool,
    pub dropped: AtomicU64,
}

impl Subscriber {
    pub fn new(capacity: usize) -> (Arc<Self>, Receiver<Packet>) {
        let (tx, rx) = crossbeam_channel::bounded(capacity);
        (Arc::new(Self { tx, needs_key: AtomicBool::new(true), dropped: AtomicU64::new(0) }), rx)
    }
    pub fn depth(&self) -> usize {
        self.tx.len()
    }
}

#[derive(Default)]
pub struct Bus {
    subs: Mutex<Vec<Arc<Subscriber>>>,
}

impl Bus {
    pub fn subscribe(&self, s: Arc<Subscriber>) {
        self.subs.lock().push(s);
    }
    pub fn unsubscribe(&self, s: &Arc<Subscriber>) {
        self.subs.lock().retain(|x| !Arc::ptr_eq(x, s));
    }
    pub fn has_subscribers(&self) -> bool {
        !self.subs.lock().is_empty()
    }

    /// Returns true if some video subscriber needs a keyframe (caller may force an IDR).
    pub fn publish(&self, pkt: &Packet) -> bool {
        let mut want_key = false;
        let subs = self.subs.lock().clone();
        for s in subs.iter() {
            if pkt.is_video && s.needs_key.load(Ordering::Relaxed) {
                if !pkt.is_key() {
                    s.dropped.fetch_add(1, Ordering::Relaxed);
                    want_key = true;
                    continue;
                }
                s.needs_key.store(false, Ordering::Relaxed);
            }
            match s.tx.try_send(pkt.clone_ref()) {
                Ok(()) => {}
                Err(TrySendError::Full(_)) | Err(TrySendError::Disconnected(_)) => {
                    s.dropped.fetch_add(1, Ordering::Relaxed);
                    if pkt.is_video {
                        s.needs_key.store(true, Ordering::Relaxed);
                        want_key = true;
                    }
                }
            }
        }
        want_key
    }
}
