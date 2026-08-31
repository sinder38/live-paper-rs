use std::process::{ChildStdin, Command, Stdio};
use std::time::{Duration, Instant};

use calloop::channel;
use log::{error, info, warn};

use crate::config::Config;
use crate::ipc::{RenderCmd, RenderEvent, State, write_line};

/// How long a retiring child gets to exit on its own before it is killed
const QUIT_GRACE: Duration = Duration::from_secs(2);

/// What a child's reader thread reports back
pub enum ChildMsg {
    Event {
        id: u64,
        event: RenderEvent,
    },
    /// stdout reached EOF, so the process is gone
    Gone {
        id: u64,
    },
}

/// A running renderer process
pub struct Child {
    pub id: u64,
    proc: std::process::Child,
    stdin: ChildStdin,
    started: Instant,
    /// The last status this child reported
    pub status: Option<State>,
}

impl Child {
    /// Start a renderer and wire its stdout to `sender`
    /// Renderer is this same binary run with `--renderer`, so installation still ships 2 executables
    pub fn spawn(
        id: u64,
        config: &Config,
        video: &str,
        sender: channel::Sender<ChildMsg>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let exe = std::env::current_exe()?;
        let mut proc = Command::new(&exe)
            .arg("--renderer")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            // Renderer logs land on the daemon's stderr
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|e| format!("failed to start the renderer {}: {e}", exe.display()))?;

        let stdin = proc.stdin.take().ok_or("renderer stdin was not piped")?;
        let stdout = proc.stdout.take().ok_or("renderer stdout was not piped")?;

        // A thread, because std cannot put a pipe into non-blocking mode
        std::thread::spawn(move || {
            use std::io::BufRead;
            let reader = std::io::BufReader::new(stdout);
            for line in reader.lines() {
                let Ok(line) = line else { break };
                match serde_json::from_str::<RenderEvent>(&line) {
                    Ok(event) => {
                        if sender.send(ChildMsg::Event { id, event }).is_err() {
                            return;
                        }
                    }
                    Err(e) => error!("Unparsable event from renderer: {e}"),
                }
            }
            let _ = sender.send(ChildMsg::Gone { id });
        });

        let mut child = Self {
            id,
            proc,
            stdin,
            started: Instant::now(),
            status: None,
        };
        info!("Started renderer {} playing {video}", child.pid());

        child.send(&RenderCmd::Init {
            config: Box::new(config.clone()),
            video: video.to_string(),
        });
        Ok(child)
    }

    pub fn pid(&self) -> u32 {
        self.proc.id()
    }

    pub fn uptime(&self) -> Duration {
        self.started.elapsed()
    }

    pub fn send(&mut self, cmd: &RenderCmd) {
        if let Err(e) = write_line(&mut self.stdin, cmd) {
            error!("Failed to send {cmd:?} to renderer {}: {e}", self.pid());
        }
    }

    pub fn memory_kb(&self) -> Option<u64> {
        let status = std::fs::read_to_string(format!("/proc/{}/status", self.pid())).ok()?;
        parse_vm_rss(&status)
    }

    /// Reap an already-dead child, reporting how it went
    pub fn reap(&mut self) -> std::io::Result<std::process::ExitStatus> {
        self.proc.wait()
    }

    /// Ask the child to leave, then kill it if it will not. Blocks for at most
    /// `QUIT_GRACE`, which only happens while a replacement is already on screen
    pub fn shutdown(mut self) {
        self.send(&RenderCmd::Quit);
        // Closing stdin makes the child's reader thread see EOF even if the
        // command above was lost
        drop(self.stdin);

        let deadline = Instant::now() + QUIT_GRACE;
        loop {
            match self.proc.try_wait() {
                Ok(Some(_)) => return,
                Ok(None) => {}
                Err(e) => {
                    error!("Failed to wait on renderer {}: {e}", self.proc.id());
                    return;
                }
            }

            if Instant::now() >= deadline {
                warn!(
                    "Renderer {} did not exit in {QUIT_GRACE:?}, killing it",
                    self.proc.id()
                );
                let _ = self.proc.kill();
                let _ = self.proc.wait();
                return;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// Kill without waiting for a clean exit
    pub fn kill(mut self) {
        let _ = self.proc.kill();
        let _ = self.proc.wait();
    }
}

fn parse_vm_rss(status: &str) -> Option<u64> {
    status
        .lines()
        .find_map(|l| l.strip_prefix("VmRSS:"))?
        .split_whitespace()
        .next()?
        .parse()
        .ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_vm_rss() {
        let status = "Name:\tlive-paper\nVmPeak:\t  900 kB\nVmRSS:\t  286404 kB\nThreads:\t8\n";
        assert_eq!(parse_vm_rss(status), Some(286_404));
    }

    #[test]
    fn missing_vm_rss_is_none() {
        assert_eq!(parse_vm_rss("Name:\tlive-paper\nThreads:\t8\n"), None);
    }
}
