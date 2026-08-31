#[cfg(not(feature = "auto-restart"))]
use std::time::Instant;

use super::child::Child;

#[cfg(feature = "auto-restart")]
pub use enabled::{Policy, RestartConfig};

#[cfg(not(feature = "auto-restart"))]
pub use disabled::{Policy, RestartConfig};

#[cfg(feature = "auto-restart")]
mod enabled {
    use std::time::{Duration, Instant};

    use log::{debug, error};
    use serde::{Deserialize, Serialize};

    use super::Child;

    /// How often the renderer's memory is checked
    const MEMORY_POLL: Duration = Duration::from_secs(60);

    /// Restart section of the config
    #[derive(Debug, Serialize, Deserialize, Default, Clone, PartialEq)]
    #[serde(default)]
    pub struct RestartConfig {
        /// Replace renderer this often
        pub every: Option<String>,
        /// Replace it once it passes this much memory, in MiB. Unset means never
        pub above_memory_mb: Option<u64>,
    }

    impl RestartConfig {
        /// Parsed `every`; an unusable value is reported and ignored
        fn interval(&self) -> Option<Duration> {
            let raw = self.every.as_deref()?;
            match parse_duration(raw) {
                Ok(interval) => Some(interval),
                Err(e) => {
                    error!("Ignoring restart.every = \"{raw}\": {e}");
                    None
                }
            }
        }
    }

    /// When to replace the renderer
    pub struct Policy {
        interval: Option<Duration>,
        limit_mb: Option<u64>,
        /// When the scheduled restart is due
        due_at: Option<Instant>,
        /// When to sample memory next
        poll_at: Option<Instant>,
    }

    impl Policy {
        pub fn new(config: &RestartConfig) -> Self {
            let now = Instant::now();
            let interval = config.interval();
            let limit_mb = config.above_memory_mb;

            Self {
                interval,
                limit_mb,
                due_at: interval.map(|i| now + i),
                poll_at: limit_mb.map(|_| now + MEMORY_POLL),
            }
        }

        /// When the daemon should next call
        pub fn deadline(&self) -> Option<Instant> {
            [self.due_at, self.poll_at].into_iter().flatten().min()
        }

        /// Why the renderer should be replaced right now, if it should be
        pub fn due(&mut self, child: Option<&Child>) -> Option<String> {
            let now = Instant::now();

            if self.due_at.is_some_and(|at| now >= at) {
                let interval = self.interval.expect("a due time implies an interval");
                return Some(format!("{interval:?} since the last restart"));
            }

            if self.poll_at.is_some_and(|at| now >= at) {
                self.poll_at = Some(now + MEMORY_POLL);
                return self.over_limit(child?);
            }

            None
        }

        fn over_limit(&self, child: &Child) -> Option<String> {
            let limit_mb = self.limit_mb?;
            let used_mb = child.memory_kb()? / 1024;

            debug!("Renderer {} is using {used_mb} MiB", child.pid());
            (used_mb >= limit_mb).then(|| format!("{used_mb} MiB is over the {limit_mb} MiB limit"))
        }

        ///Sstart counting again
        pub fn restarted(&mut self) {
            self.due_at = self.interval.map(|i| Instant::now() + i);
        }
    }

    /// Parse "90s" / "30m" / "6h" / "2d"
    fn parse_duration(raw: &str) -> Result<Duration, String> {
        let raw = raw.trim();
        let split = raw.find(|c: char| !c.is_ascii_digit()).unwrap_or(raw.len());
        let (digits, unit) = raw.split_at(split);

        let n: u64 = digits
            .parse()
            .map_err(|_| format!("expected a number followed by s/m/h/d, got \"{raw}\""))?;

        let secs = match unit.trim() {
            "" | "s" => n,
            "m" => n * 60,
            "h" => n * 60 * 60,
            "d" => n * 60 * 60 * 24,
            other => {
                return Err(format!(
                    "unknown time unit \"{other}\", expected s, m, h or d"
                ));
            }
        };

        if secs == 0 {
            return Err("must be greater than zero".to_string());
        }
        Ok(Duration::from_secs(secs))
    }
}

#[cfg(not(feature = "auto-restart"))]
mod disabled {
    use log::warn;
    use serde::{Deserialize, Serialize};

    use super::{Child, Instant};

    /// The same `[restart]` section, so a config file stays readable by builds
    /// with and without the feature. Both fields are inert here
    #[derive(Debug, Serialize, Deserialize, Default, Clone, PartialEq)]
    #[serde(default)]
    pub struct RestartConfig {
        pub every: Option<String>,
        pub above_memory_mb: Option<u64>,
    }

    /// Built without `auto-restart`: the renderer is only ever replaced when
    /// asked, by `live-paper restart` or a config change that needs it
    pub struct Policy;

    impl Policy {
        pub fn new(config: &RestartConfig) -> Self {
            // Say so rather than quietly doing nothing with it
            if config.every.is_some() || config.above_memory_mb.is_some() {
                warn!(
                    "[restart] is set, but this build has no `auto-restart` feature; \
                     ignoring it. Use `live-paper restart` to replace the renderer"
                );
            }
            Self
        }

        pub fn deadline(&self) -> Option<Instant> {
            None
        }

        pub fn due(&mut self, _child: Option<&Child>) -> Option<String> {
            None
        }

        pub fn restarted(&mut self) {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_durations() {
        assert_eq!(parse_duration("90s").unwrap(), Duration::from_secs(90));
        assert_eq!(parse_duration("30m").unwrap(), Duration::from_secs(1800));
        assert_eq!(parse_duration("6h").unwrap(), Duration::from_secs(21600));
        assert_eq!(parse_duration("2d").unwrap(), Duration::from_secs(172_800));
        // bare number is seconds
        assert_eq!(parse_duration("45").unwrap(), Duration::from_secs(45));
    }

    #[test]
    fn rejects_bad_durations() {
        for bad in ["", "h", "6y", "abc", "0s", "-5m"] {
            assert!(parse_duration(bad).is_err(), "{bad} should not parse");
        }
    }

    #[test]
    fn unset_config_never_fires() {
        let mut policy = Policy::new(&RestartConfig::default());
        assert!(policy.deadline().is_none());
        assert!(policy.due(None).is_none());
    }

    #[test]
    fn a_schedule_comes_due() {
        let mut policy = Policy::new(&RestartConfig {
            every: Some("1s".into()),
            above_memory_mb: None,
        });
        assert!(policy.deadline().is_some());
        assert!(policy.due(None).is_none(), "not due yet");

        // Reach back in time rather than sleeping through the interval
        policy.due_at = Some(Instant::now() - Duration::from_millis(1));
        assert!(policy.due(None).is_some());

        policy.restarted();
        assert!(policy.due(None).is_none(), "the clock restarts");
    }

    #[test]
    fn an_unparsable_interval_is_ignored() {
        let policy = Policy::new(&RestartConfig {
            every: Some("whenever".into()),
            above_memory_mb: None,
        });
        assert!(policy.deadline().is_none());
    }
}
