use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

/// How often config file is checked
const POLL: Duration = Duration::from_secs(1);

/// Notices config file edits by polling its mtime
pub struct ConfigWatch {
    path: PathBuf,
    /// mtime at last poll, `None` while file is missing
    seen: Option<SystemTime>,
    /// mtime moved; reload once it settles
    changed: bool,
    poll_at: Instant,
}

impl ConfigWatch {
    pub fn new(path: PathBuf) -> Self {
        Self {
            seen: mtime(&path),
            path,
            changed: false,
            poll_at: Instant::now() + POLL,
        }
    }

    /// When daemon should next call due()
    pub fn deadline(&self) -> Instant {
        self.poll_at
    }

    /// True once file changed and then held still for one poll, so
    /// half-written saves are not loaded
    pub fn due(&mut self) -> bool {
        let now = Instant::now();
        if now < self.poll_at {
            return false;
        }
        self.poll_at = now + POLL;

        let mtime = mtime(&self.path);
        if mtime != self.seen {
            self.seen = mtime;
            self.changed = true;
            return false;
        }
        std::mem::take(&mut self.changed)
    }
}

fn mtime(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}
