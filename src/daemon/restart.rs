use log::debug;

use crate::config::RestartConfig;
use std::time::{Duration, Instant};

use super::child::Child;

/// How often renderer memory is sampled
const POLL: Duration = Duration::from_secs(60);

const MIB: u64 = 1024 * 1024;

/// When to replace renderer
/// Every field is `None` in a build without `auto-restart` feat
#[derive(Default)]
pub struct RestartPolicy {
    delta_limit_mb: Option<u64>,
    /// When to sample memory next
    poll_at: Option<Instant>,
    /// Application used memory at the start
    floor: Option<u64>,
}

impl RestartPolicy {
    pub fn new(config: &RestartConfig) -> Self {
        #[cfg(not(feature = "auto-restart"))]
        {
            if config.passed_delta_mb.is_some() {
                log::warn!(
                    "restart.passed_delta_mb is set, but this build has no \
                     `auto-restart` feature; ignoring it. Use `live-paper restart` \
                     to replace renderer"
                );
            }
            Self::default()
        }

        #[cfg(feature = "auto-restart")]
        {
            let now = Instant::now();
            Self {
                delta_limit_mb: config.passed_delta_mb,
                poll_at: config.passed_delta_mb.map(|_| now + POLL),
                floor: None, // Set during init_policy()
            }
        }
    }

    /// Record memory floor (baseline + video size) the delta is measured from
    pub fn init_policy(&mut self, video: &str, baseline_mb: u64) {
        // Streams have no size on disk; only local files count
        let video_mb = std::fs::metadata(video).map_or(0, |m| m.len() / MIB);
        self.floor = Some(baseline_mb + video_mb);
        debug!("Floor Mb: {:?}", self.floor);
    }

    /// When daemon should next call due()
    pub fn deadline(&self) -> Option<Instant> {
        self.poll_at
    }

    /// Why renderer should be replaced right now, if it should be
    pub fn due(&mut self, child: Option<&Child>) -> Option<String> {
        let now = Instant::now();
        if self.poll_at.is_none_or(|at| now < at) {
            return None;
        }
        self.poll_at = Some(now + POLL);

        let delta_limit_mb = self.delta_limit_mb?;
        let child = child?;
        let used_mb = child.memory_kb()? / 1024;
        let floor_mb = self.floor.expect("floor should have been set");

        debug!(
            "Renderer {} is using {used_mb} MiB, floor {floor_mb} MiB",
            child.pid()
        );

        let delta_mb = used_mb.saturating_sub(floor_mb);
        if delta_mb >= delta_limit_mb {
            Some(format!(
                "{delta_mb} MiB grown, over {delta_limit_mb} MiB limit"
            ))
        } else {
            None
        }
    }
}
