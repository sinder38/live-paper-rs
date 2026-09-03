use std::ffi::c_void;

use libmpv2::Mpv;
use libmpv2::render::{OpenGLInitParams, RenderContext, RenderParam, RenderParamApiType};

use khronos_egl as egl;
use log::error;

use crate::backend::BackendCtx;
use crate::config::PlayerConfig;

/// libmpv has a leak, which is unsustainable for a live paper.
/// mpv-player/mpv#17303
#[cfg(feature = "libmpv-restart")]
const MPV_FENCE_LEAK_FIXED: (u32, u32, u32) = (0, 42, 0);

/// Wraps an mpv instance and its OpenGL render context
/// The `Mpv` is intentionally leaked to obtain a static ref
pub struct Player {
    mpv: &'static Mpv,
    render: Option<RenderContext<'static>>,
    path: String,
    #[cfg(feature = "libmpv-restart")]
    config: PlayerConfig,
    #[cfg(feature = "libmpv-restart")]
    debug: bool,
    /// True if this mpv build predates mpv-player/mpv#17303
    #[cfg(feature = "libmpv-restart")]
    needs_restart: bool,
}

fn get_proc_address(egl_instance: &egl::Instance<egl::Static>, name: &str) -> *mut c_void {
    egl_instance
        .get_proc_address(name)
        .map(|f| f as *mut c_void)
        .unwrap_or(std::ptr::null_mut())
}

/// Create and configure a leaked, `'static` mpv instance from `config`
fn create_mpv(
    config: &PlayerConfig,
    debug: bool,
) -> Result<&'static Mpv, Box<dyn std::error::Error>> {
    // `vo=libmpv` MUST be set before mpv initializes, or mpv opens
    // its own window (a normal `class=mpv` toplevel) and ignores my render context
    let mpv = Box::leak(Box::new(Mpv::with_initializer(|init| {
        init.set_option("vo", "libmpv")
    })?));

    // Ref: https://mpv.io/manual/master/#options
    // TODO: test variant with initializer
    if debug || std::env::var("LP_DEBUG").is_ok() {
        mpv.set_property("terminal", "yes")?; // keep io
        mpv.set_property("msg-level", "all=status")?; // status msgs
    }
    mpv.set_property("loop-file", "inf")?; // a wallpaper never stops
    mpv.set_property("hwdec", config.hwdec.as_str())?; // GPU decode when possible
    mpv.set_property("mute", config.mute)?;
    mpv.set_property("speed", config.speed)?;
    // 1.0 zooms and crops the overflow; `0.0` letterboxes it instead
    mpv.set_property("panscan", if config.fill { 1.0 } else { 0.0 })?;

    // Free-form passthrough for anything not modeled above
    for (name, value) in &config.mpv_options {
        if let Err(e) = mpv.set_property(name.as_str(), value.as_str()) {
            eprintln!("Failed to set mpv option {name}={value}: {e}");
        }
    }

    Ok(mpv)
}

/// Check for mpv-player/mpv#17303.
/// Unknown/unparseable versions are treated as unfixed
#[cfg(feature = "libmpv-restart")]
fn mpv_has_fence_leak_fix(mpv: &Mpv) -> bool {
    let Ok(version) = mpv.get_property::<String>("mpv-version") else {
        return false;
    };
    let Some(numbers) = version.split_whitespace().nth(1) else {
        return false;
    };
    // e.g. "v0.41.0"
    let numbers = numbers.trim_start_matches(['v', 'V']);
    let mut parts = numbers.split('.').filter_map(|n| n.parse::<u32>().ok());
    let Some(major) = parts.next() else {
        return false;
    };
    let minor = parts.next().unwrap_or(0);
    let patch = parts.next().unwrap_or(0);
    (major, minor, patch) >= MPV_FENCE_LEAK_FIXED
}

impl Player {
    pub fn new(
        path: impl ToString,
        config: &PlayerConfig,
        debug: bool,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let path = path.to_string();

        // Without this check a missing file just plays as a black screen
        if !path.contains("://") && !std::path::Path::new(&path).exists() {
            return Err(format!("video file not found: {path}").into());
        }

        let mpv = create_mpv(config, debug)?;

        Ok(Self {
            mpv,
            render: None,
            path,
            #[cfg(feature = "libmpv-restart")]
            config: config.clone(),
            #[cfg(feature = "libmpv-restart")]
            debug,
            #[cfg(feature = "libmpv-restart")]
            needs_restart: !mpv_has_fence_leak_fix(mpv),
        })
    }
}

#[cfg(feature = "libmpv-restart")]
impl Player {
    /// True if periodic restarts should run to work around mpv-player/mpv#15099
    pub fn needs_restart(&self) -> bool {
        self.needs_restart
    }

    /// Tear down and recreate the mpv instance and render context, reclaiming the
    /// GL fences/buffers a pre-#17303 mpv never frees on its own.
    pub fn restart(&mut self, ctx: BackendCtx) -> Result<(), Box<dyn std::error::Error>> {
        // Drop the render context first: it borrows `self.mpv` and must not outlive it.
        self.render = None;
        // Safety: `self.mpv` was leaked via `Box::leak` in `create_mpv`, and the only
        // borrow of it (the render context above) was just dropped, so nothing else
        // references it.
        drop(unsafe { Box::from_raw(self.mpv as *const Mpv as *mut Mpv) });

        self.mpv = create_mpv(&self.config, self.debug)?;
        self.init(ctx)
    }
}

impl Player {
    /// Create the GL render context and start playback
    pub fn init(&mut self, ctx: BackendCtx) -> Result<(), Box<dyn std::error::Error>> {
        let render = self.mpv.create_render_context(vec![
            RenderParam::ApiType(RenderParamApiType::OpenGl),
            RenderParam::InitParams(OpenGLInitParams {
                get_proc_address,
                // Instance<Static> is ZST, so building a fresh owned one is free
                ctx: egl::Instance::new(egl::Static),
            }),
            RenderParam::WaylandDisplay(ctx.display_ptr as *const c_void),
        ])?;

        self.render = Some(render);
        //TODO: use ref later
        self.mpv.command("loadfile", &[&self.path])?;
        Ok(())
    }

    /// Draw the current video frame. mpv drives its own GL, so gl,time are unused
    pub fn render(&mut self, _gl: &glow::Context, width: i32, height: i32, _time: u32) {
        if let Some(render) = &self.render {
            // Reversed: flip for normal, no flip for flipped
            let flip = true;
            let _ = render.render::<()>(0, width, height, flip);
        }
    }

    pub fn pause(&mut self) {
        if let Err(e) = self.mpv.set_property("pause", true) {
            error!("Failed to pause mpv: {e}");
        }
    }

    pub fn resume(&mut self) {
        if let Err(e) = self.mpv.set_property("pause", false) {
            error!("Failed to resume mpv: {e}");
        }
    }
}
