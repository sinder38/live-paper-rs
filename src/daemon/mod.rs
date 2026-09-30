use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use calloop::generic::Generic;
use calloop::{EventLoop, Interest, LoopHandle, Mode, PostAction, channel};
use log::{debug, error, info, warn};

use crate::config::{self, Config};
use crate::ipc::{
    RenderCmd, RenderEvent, Request, Response, State, read_request, socket_path, write_line,
};

mod child;
mod restart;
mod watch;

use child::{Child, ChildMsg};
use restart::RestartPolicy;
use watch::ConfigWatch;

/// How long a replacement gets to draw its first frame before we give up on it
const READY_TIMEOUT: Duration = Duration::from_secs(5);
/// Wait before respawning a child that died on its own
const RESPAWN_BACKOFF: Duration = Duration::from_secs(1);
/// Consecutive crashes before we stop respawning and wait for a `restart`
const MAX_FAILURES: u32 = 5;
/// A client that cannot manage a request in this long is not worth waiting for
const CLIENT_TIMEOUT: Duration = Duration::from_secs(5);

pub struct Daemon {
    config: Config,
    /// The `-c` path, so `reload` re-reads the same file
    config_path: Option<PathBuf>,
    /// The video we want playing
    video: String,
    manual_pause: bool,

    /// Child that owns the surface right now
    current: Option<Child>,
    /// A replacement that has not drawn its first frame yet
    pending: Option<Child>,
    /// When to give up on `pending`
    pending_deadline: Option<Instant>,

    next_id: u64,
    sender: channel::Sender<ChildMsg>,

    /// When renderer should be replaced
    policy: RestartPolicy,
    /// Memory used as baseline
    baseline_mb: Option<u64>,
    /// Config file watcher, `None` when `auto_reload` is off
    watch: Option<ConfigWatch>,

    /// Consecutive crashes; respawning stops once this hits `MAX_FAILURES`
    failures: u32,
    /// When a crashed child may be respawned
    respawn_at: Option<Instant>,

    exit: bool,
}

impl Daemon {
    fn new(
        config: Config,
        config_path: Option<PathBuf>,
        video: String,
    ) -> (Self, channel::Channel<ChildMsg>) {
        let (sender, receiver) = channel::channel();
        let policy = RestartPolicy::new(&config.restart);

        let mut daemon = Self {
            config,
            config_path,
            video,
            manual_pause: false,
            current: None,
            pending: None,
            pending_deadline: None,
            next_id: 0,
            sender,
            policy,
            baseline_mb: None,
            watch: None,
            failures: 0,
            respawn_at: None,
            exit: false,
        };
        daemon.sync_watch();
        (daemon, receiver)
    }

    /// Start or stop watching config file to match `auto_reload`
    fn sync_watch(&mut self) {
        if !self.config.auto_reload {
            self.watch = None;
        } else if self.watch.is_none() {
            let path = self.config_path.clone().unwrap_or_else(config::config_path);
            debug!("Watching config at {}", path.display());
            self.watch = Some(ConfigWatch::new(path));
        }
    }

    fn spawn(&mut self) -> Result<Child, Box<dyn std::error::Error>> {
        let id = self.next_id;
        self.next_id += 1;

        let mut child = Child::spawn(id, &self.config, &self.video, self.sender.clone())?;
        if self.manual_pause {
            child.send(&RenderCmd::SetManualPause { paused: true });
        }
        Ok(child)
    }

    /// Bring up a replacement
    fn restart(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        if self.pending.is_some() {
            return Err("a restart is already in progress".into());
        }

        let child = self.spawn()?;
        self.pending_deadline = Some(Instant::now() + READY_TIMEOUT);
        self.pending = Some(child);
        Ok(())
    }

    /// Put replacement on screen and remove child
    fn promote(&mut self) {
        let Some(new) = self.pending.take() else {
            return;
        };
        self.pending_deadline = None;
        self.failures = 0;

        let pid = new.pid();
        if let Some(old) = self.current.replace(new) {
            debug!("Renderer {pid} is ready, retiring {}", old.pid());
            old.shutdown();
        }
    }

    /// Push changes to child on screen and to a pending-one to avoid desync
    fn broadcast(&mut self, cmd: RenderCmd) {
        for child in [self.current.as_mut(), self.pending.as_mut()]
            .into_iter()
            .flatten()
        {
            child.send(&cmd);
        }
    }

    fn handle_child_msg(&mut self, msg: ChildMsg) {
        match msg {
            ChildMsg::Event { id, event } => self.handle_child_event(id, event),
            ChildMsg::Gone { id } => self.handle_child_gone(id),
        }
    }

    fn handle_child_event(&mut self, id: u64, event: RenderEvent) {
        match event {
            RenderEvent::Ready => {
                if self.pending.as_ref().is_some_and(|c| c.id == id) {
                    self.promote();
                } else if self.current.as_ref().is_some_and(|c| c.id == id) {
                    // First child of the session
                    self.failures = 0;
                }

                // Get app memory usage baseline
                self.baseline_mb = self
                    .current
                    .as_ref()
                    .and_then(Child::memory_kb)
                    .map(|kb| kb / 1024);
                debug!("Baseline Mb: {:?}", self.baseline_mb);

                // Set policy baseline
                self.policy
                    .init_policy(&self.video, self.baseline_mb.unwrap_or(0));
            }
            RenderEvent::Status(state) => {
                for child in [self.current.as_mut(), self.pending.as_mut()]
                    .into_iter()
                    .flatten()
                {
                    if child.id == id {
                        child.status = Some(*state);
                        return;
                    }
                }
            }
            RenderEvent::Error { message } => error!("Renderer {id}: {message}"),
        }
    }

    fn handle_child_gone(&mut self, id: u64) {
        // A replacement that died before drawing, keep existing on screen
        if self.pending.as_ref().is_some_and(|c| c.id == id) {
            let child = self.pending.take().expect("just matched");
            self.pending_deadline = None;
            error!(
                "Replacement renderer {} exited before drawing; keeping the current one",
                child.pid()
            );
            child.kill();
            return;
        }

        if self.current.as_ref().is_none_or(|c| c.id != id) {
            // nothing to do
            return;
        }

        let mut child = self.current.take().expect("just matched");
        match child.reap() {
            // A clean exit means the layer surface closed
            Ok(status) if status.success() => {
                info!("Renderer exited cleanly, shutting down");
                self.exit = true;
            }
            Ok(status) => {
                self.failures += 1;
                error!(
                    "Renderer died ({status}), attempt {} of {MAX_FAILURES}",
                    self.failures
                );
                self.schedule_respawn();
            }
            Err(e) => {
                self.failures += 1;
                error!("Failed to reap the renderer: {e}");
                self.schedule_respawn();
            }
        }
    }

    fn schedule_respawn(&mut self) {
        if self.failures >= MAX_FAILURES {
            error!(
                "Renderer failed {MAX_FAILURES} times in a row; not respawning. \
                 Fix the config and run `live-paper restart`"
            );
            self.respawn_at = None;
            return;
        }
        self.respawn_at = Some(Instant::now() + RESPAWN_BACKOFF);
    }

    /// Everything time-based: respawn backoff, ready timeout, and whatever the
    /// restart policy has decided. Called after every dispatch
    fn tick(&mut self) {
        let now = Instant::now();

        if self.respawn_at.is_some_and(|at| now >= at) {
            self.respawn_at = None;
            match self.spawn() {
                Ok(child) => self.current = Some(child),
                Err(e) => {
                    self.failures += 1;
                    error!("Failed to respawn the renderer: {e}");
                    self.schedule_respawn();
                }
            }
        }

        if self.pending_deadline.is_some_and(|at| now >= at)
            && let Some(child) = self.pending.take()
        {
            self.pending_deadline = None;
            self.failures += 1;
            error!(
                "Replacement renderer {} did not draw within {READY_TIMEOUT:?}; \
                 keeping the current one",
                child.pid()
            );
            child.kill();
        }

        // Config watcher
        if self.watch.as_mut().is_some_and(ConfigWatch::due) {
            match self.handle_reload() {
                Response::Error { message } => error!("{message}"),
                _ => info!("Config file changed, reloaded"),
            }
        }

        // Restart watcher
        if let Some(reason) = self.policy.due(self.current.as_ref()) {
            info!("Replacing the renderer: {reason}");
            if let Err(e) = self.restart() {
                warn!("Automatic restart skipped: {e}");
            }
        }
    }

    /// How long the event loop may sleep before `tick` has something to do
    fn next_deadline(&self) -> Option<Duration> {
        let now = Instant::now();
        [
            self.respawn_at,
            self.pending_deadline,
            self.policy.deadline(),
            self.watch.as_ref().map(ConfigWatch::deadline),
        ]
        .into_iter()
        .flatten()
        .map(|at| at.saturating_duration_since(now))
        .min()
    }

    fn state(&self) -> State {
        let child = self.current.as_ref();
        let mut state = child
            .and_then(|c| c.status.clone())
            .unwrap_or_else(|| State {
                video: self.video.clone(),
                backend: self.config.backend,
                paused: self.manual_pause,
                ..State::default()
            });

        state.renderer_pid = child.map(Child::pid);
        state.renderer_uptime_secs = child.map_or(0, |c| c.uptime().as_secs());
        state.renderer_rss_kb = child.and_then(Child::memory_kb);
        state
    }

    fn handle(&mut self, req: Request) -> Response {
        match req {
            Request::Set {
                video,
                speed,
                mute,
                fill,
                output,
            } => self.handle_set(video, speed, mute, fill, output),
            Request::Query => Response::State(Box::new(self.state())),
            Request::Pause => self.set_manual_pause(true),
            Request::Resume => self.set_manual_pause(false),
            Request::Toggle => self.set_manual_pause(!self.manual_pause),
            Request::Reload => self.handle_reload(),
            Request::Restart => match self.restart() {
                Ok(()) => Response::msg("restarting the renderer"),
                Err(e) => Response::error(e),
            },
            Request::Kill => {
                self.exit = true;
                Response::msg("stopping")
            }
        }
    }

    fn handle_set(
        &mut self,
        video: Option<String>,
        speed: Option<f64>,
        mute: Option<bool>,
        fill: Option<bool>,
        output: Option<String>,
    ) -> Response {
        if let Some(output) = output {
            return Response::error(format!(
                "per-output wallpapers are not supported yet, cannot target {output}"
            ));
        }

        if let Some(video) = video {
            // Resolve here so the error reaches the client rather than only the
            // renderer's log
            let video = match resolve_video(&video) {
                Ok(video) => video,
                Err(e) => return Response::error(e),
            };
            // A bigger file raises the floor the delta is measured from
            self.policy
                .init_policy(&video, self.baseline_mb.unwrap_or(0));
            self.video = video.clone();
            self.broadcast(RenderCmd::SetVideo { path: video });
        }
        if let Some(speed) = speed {
            self.config.player.speed = speed;
            self.broadcast(RenderCmd::SetSpeed { speed });
        }
        if let Some(mute) = mute {
            self.config.player.mute = mute;
            self.broadcast(RenderCmd::SetMute { mute });
        }
        if let Some(fill) = fill {
            self.config.player.fill = fill;
            self.broadcast(RenderCmd::SetFill { fill });
        }
        Response::ok()
    }

    fn set_manual_pause(&mut self, paused: bool) -> Response {
        self.manual_pause = paused;
        self.broadcast(RenderCmd::SetManualPause { paused });
        Response::msg(if paused { "paused" } else { "resumed" })
    }

    fn handle_reload(&mut self) -> Response {
        let new = match Config::load(self.config_path.clone()) {
            Ok(new) => new,
            Err(e) => {
                return Response::error(format!("reload failed, keeping the old config: {e}"));
            }
        };

        let restart_needed = self.config.needs_restart(&new);

        // Live player tweaks go straight through; video changes only when file's
        // `path` changed, so CLI or `set` video survives unrelated edits
        let video = new
            .player
            .path
            .clone()
            .filter(|p| Some(p) != self.config.player.path.as_ref());

        let mut policy = RestartPolicy::new(&new.restart);
        let next_video = video.as_ref().unwrap_or(&self.video);
        policy.init_policy(next_video, self.baseline_mb.unwrap_or(0));

        let speed = (new.player.speed != self.config.player.speed).then_some(new.player.speed);
        let mute = (new.player.mute != self.config.player.mute).then_some(new.player.mute);
        let fill = (new.player.fill != self.config.player.fill).then_some(new.player.fill);

        self.config = new;
        self.policy = policy;
        self.sync_watch();

        if restart_needed {
            // The new config is already stored, so the replacement picks it up
            return match self.restart() {
                Ok(()) => Response::msg("config reloaded, restarting the renderer"),
                Err(e) => Response::error(e),
            };
        }

        self.handle_set(video, speed, mute, fill, None);
        Response::msg("config reloaded")
    }
}

/// Reject a missing file here rather than letting it play as a black screen
fn resolve_video(video: &str) -> Result<String, String> {
    if video.contains("://") {
        return Ok(video.to_string());
    }

    let path = Path::new(video);
    if !path.exists() {
        return Err(format!("video file not found: {video}"));
    }
    // The renderer has the same working directory today, but it will not once
    // the daemon is started from a session manager
    std::fs::canonicalize(path)
        .map(|p| p.to_string_lossy().into_owned())
        .map_err(|e| format!("cannot resolve {video}: {e}"))
}

/// Answer one client. Errors here are the client's problem, not the daemon's
fn serve(daemon: &mut Daemon, mut stream: UnixStream) {
    if let Err(e) = stream.set_read_timeout(Some(CLIENT_TIMEOUT)) {
        error!("Failed to set a read timeout on a client: {e}");
        return;
    }
    let _ = stream.set_write_timeout(Some(CLIENT_TIMEOUT));

    let request = match read_request(&stream) {
        Ok(Some(request)) => request,
        Ok(None) => return,
        Err(e) => {
            warn!("Unreadable request: {e}");
            let _ = write_line(&mut stream, &Response::error(e));
            return;
        }
    };

    debug!("Request: {request:?}");
    let response = daemon.handle(request);
    if let Err(e) = write_line(&mut stream, &response) {
        warn!("Failed to answer a client: {e}");
    }
}

/// Removes the socket file when the daemon goes down
struct SocketGuard(PathBuf);

impl Drop for SocketGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// Bind the socket, clearing one left behind by a daemon that was killed
fn bind(path: &Path) -> Result<UnixListener, Box<dyn std::error::Error>> {
    match UnixListener::bind(path) {
        Ok(listener) => return Ok(listener),
        Err(e) if e.kind() != std::io::ErrorKind::AddrInUse => {
            return Err(format!("cannot bind {}: {e}", path.display()).into());
        }
        Err(_) => {}
    }

    // Something is at that path. If it still answers, we are the second daemon
    if UnixStream::connect(path).is_ok() {
        return Err(format!(
            "another live-paper daemon is already running on {}",
            path.display()
        )
        .into());
    }

    warn!("Removing a stale socket at {}", path.display());
    std::fs::remove_file(path)?;
    UnixListener::bind(path).map_err(|e| format!("cannot bind {}: {e}", path.display()).into())
}

/// Run the daemon until a client kills it or the compositor goes away
pub fn run(
    config: Config,
    config_path: Option<PathBuf>,
    video: String,
) -> Result<(), Box<dyn std::error::Error>> {
    let path = socket_path()?;
    let listener = bind(&path)?;
    listener.set_nonblocking(true)?;
    let _guard = SocketGuard(path.clone());
    info!("Listening on {}", path.display());

    let (mut daemon, child_rx) = Daemon::new(config, config_path, video);

    let mut event_loop: EventLoop<Daemon> = EventLoop::try_new()?;
    let handle: LoopHandle<Daemon> = event_loop.handle();

    handle.insert_source(child_rx, |event, _, daemon| {
        if let channel::Event::Msg(msg) = event {
            daemon.handle_child_msg(msg);
        }
    })?;

    handle.insert_source(
        Generic::new(listener, Interest::READ, Mode::Level),
        |_, listener, daemon| {
            // Level-triggered, so drain everything that is waiting
            loop {
                match listener.accept() {
                    Ok((stream, _)) => serve(daemon, stream),
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                    Err(e) => {
                        error!("Failed to accept a client: {e}");
                        break;
                    }
                }
            }
            Ok(PostAction::Continue)
        },
    )?;

    let first = daemon.spawn()?;
    daemon.current = Some(first);

    while !daemon.exit {
        event_loop.dispatch(daemon.next_deadline(), &mut daemon)?;
        daemon.tick();
    }

    // Take the wallpaper down with us
    if let Some(child) = daemon.pending.take() {
        child.kill();
    }
    if let Some(child) = daemon.current.take() {
        child.shutdown();
    }
    Ok(())
}
