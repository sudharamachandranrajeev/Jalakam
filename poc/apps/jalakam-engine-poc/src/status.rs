use jkp_proto::{ComponentStatus, State};
use parking_lot::Mutex;

/// Thread-safe component status with a stable error code.
#[derive(Default)]
pub struct StatusCell(Mutex<ComponentStatus>);

impl StatusCell {
    pub fn set(&self, state: State, code: Option<&str>, detail: Option<String>) {
        let mut s = self.0.lock();
        let changed = s.state != state || s.code.as_deref() != code;
        *s = ComponentStatus { state, code: code.map(str::to_string), detail };
        if changed {
            crate::log!("status {:?} {} {}", state, code.unwrap_or("-"), s.detail.as_deref().unwrap_or(""));
        }
    }
    pub fn ok(&self, state: State) {
        self.set(state, None, None);
    }
    pub fn get(&self) -> ComponentStatus {
        self.0.lock().clone()
    }
    pub fn state(&self) -> State {
        self.0.lock().state
    }
}
