use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use super::{Request, Response};
use crate::APP_NAME;

/// How long a client waits for the daemon to answer
const CLIENT_TIMEOUT: Duration = Duration::from_secs(5);

/// `$XDG_RUNTIME_DIR/live-paper-<WAYLAND_DISPLAY>.sock`
/// A Wayland session cannot exist without `XDG_RUNTIME_DIR`, so its absence is an
/// error rather than a fallback to a world-writable directory.
pub fn socket_path() -> Result<PathBuf, String> {
    let dir = std::env::var_os("XDG_RUNTIME_DIR")
        .ok_or("XDG_RUNTIME_DIR is not set; is this a Wayland session?")?;
    let display = std::env::var("WAYLAND_DISPLAY").unwrap_or_else(|_| "wayland-0".to_string());
    Ok(build_socket_path(PathBuf::from(dir), &display))
}

fn build_socket_path(runtime_dir: PathBuf, display: &str) -> PathBuf {
    // WAYLAND_DISPLAY may be an absolute path, which cannot go in a file name
    let display = display.replace('/', "_");
    runtime_dir.join(format!("{APP_NAME}-{display}.sock"))
}

/// Write one JSON line
pub fn write_line<W: Write, T: Serialize>(w: &mut W, value: &T) -> std::io::Result<()> {
    let mut line = serde_json::to_vec(value)?;
    line.push(b'\n');
    w.write_all(&line)?;
    w.flush()
}

/// Read one JSON line
pub fn read_line<R: BufRead, T: for<'de> Deserialize<'de>>(
    r: &mut R,
) -> std::io::Result<Option<T>> {
    let mut line = String::new();
    if r.read_line(&mut line)? == 0 {
        return Ok(None);
    }
    serde_json::from_str(&line)
        .map(Some)
        .map_err(std::io::Error::from)
}

/// Send one request to a running daemon and wait for its answer
pub fn request(req: &Request) -> Result<Response, Box<dyn std::error::Error>> {
    let path = socket_path()?;
    let stream = UnixStream::connect(&path).map_err(|e| {
        format!(
            "cannot reach the daemon at {}: {e}\nis `live-paper-daemon` running?",
            path.display()
        )
    })?;
    stream.set_read_timeout(Some(CLIENT_TIMEOUT))?;
    stream.set_write_timeout(Some(CLIENT_TIMEOUT))?;

    let mut writer = stream.try_clone()?;
    write_line(&mut writer, req)?;
    // Half-close so the daemon's read side sees the end of the request
    let _ = stream.shutdown(std::net::Shutdown::Write);

    let mut reader = BufReader::new(stream);
    read_line(&mut reader)?.ok_or_else(|| "daemon closed the connection without answering".into())
}

/// Read a request, refusing anything absurdly long so one client cannot make the
/// daemon allocate without bound
pub fn read_request<R: Read>(r: R) -> std::io::Result<Option<Request>> {
    const MAX_REQUEST: u64 = 64 * 1024;
    let mut reader = BufReader::new(r.take(MAX_REQUEST));
    read_line(&mut reader)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::ipc::{RenderCmd, RenderEvent, State};

    fn roundtrip<T>(value: T)
    where
        T: Serialize + for<'de> Deserialize<'de> + PartialEq + std::fmt::Debug,
    {
        let mut buf = Vec::new();
        write_line(&mut buf, &value).unwrap();
        assert!(buf.ends_with(b"\n"), "lines must be newline terminated");

        let mut cursor = std::io::Cursor::new(buf);
        let back: T = read_line(&mut cursor).unwrap().unwrap();
        assert_eq!(value, back);
    }

    #[test]
    fn request_roundtrip() {
        roundtrip(Request::Query);
        roundtrip(Request::Set {
            video: Some("/x.mp4".into()),
            speed: Some(1.5),
            mute: None,
            fill: Some(false),
            output: None,
        });
    }

    #[test]
    fn response_roundtrip() {
        roundtrip(Response::msg("restarted"));
        roundtrip(Response::error("nope"));
        roundtrip(Response::State(Box::new(State {
            video: "/x.mp4".into(),
            physical: (2560, 1440),
            pause_reasons: vec!["gamemode".into()],
            ..State::default()
        })));
    }

    #[test]
    fn render_cmd_roundtrip() {
        roundtrip(RenderCmd::Quit);
        roundtrip(RenderCmd::SetVideo {
            path: "/y.mp4".into(),
        });
        roundtrip(RenderCmd::Init {
            config: Box::new(Config::default()),
            video: "/y.mp4".into(),
        });
    }

    #[test]
    fn render_event_roundtrip() {
        roundtrip(RenderEvent::Ready);
        roundtrip(RenderEvent::Status(Box::default()));
    }

    #[test]
    fn read_line_reports_closed_stream() {
        let mut empty = std::io::Cursor::new(Vec::new());
        let got: Option<Request> = read_line(&mut empty).unwrap();
        assert!(got.is_none());
    }

    #[test]
    fn socket_path_sanitises_display() {
        let run = PathBuf::from("/run/user/1000");
        assert_eq!(
            build_socket_path(run.clone(), "wayland-1"),
            PathBuf::from("/run/user/1000/live-paper-wayland-1.sock")
        );
        // An absolute WAYLAND_DISPLAY must not turn into extra path components
        assert_eq!(
            build_socket_path(run, "/run/user/1000/wayland-1"),
            PathBuf::from("/run/user/1000/live-paper-_run_user_1000_wayland-1.sock")
        );
    }
}
