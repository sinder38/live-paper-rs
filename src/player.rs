use std::ffi::c_void;

use libmpv2::Mpv;
use libmpv2::render::{OpenGLInitParams, RenderContext, RenderParam, RenderParamApiType};

use khronos_egl as egl;

/// Wraps an mpv instance and its OpenGL render context
/// The `Mpv` is intentionally leaked to obtain a static ref
pub struct Player {
    mpv: &'static Mpv,
    render: Option<RenderContext<'static>>,
    path: String,
}

fn get_proc_address(egl_instance: &egl::Instance<egl::Static>, name: &str) -> *mut c_void {
    egl_instance
        .get_proc_address(name)
        .map(|f| f as *mut c_void)
        .unwrap_or(std::ptr::null_mut())
}

impl Player {
    pub fn new(path: impl ToString) -> Result<Self, Box<dyn std::error::Error>> {
        // `vo=libmpv` MUST be set before mpv initializes, or mpv opens
        // its own window (a normal `class=mpv` toplevel) and ignores my render context
        let mpv = Box::leak(Box::new(Mpv::with_initializer(|init| {
            init.set_option("vo", "libmpv")
        })?));

        // Ref: https://mpv.io/manual/master/#options
        // TODO: test variant with initializer
        if std::env::var("LP_DEBUG").is_ok() {
            mpv.set_property("terminal", "yes")?; // keep io
            mpv.set_property("msg-level", "all=status")?; // status msgs
        }
        mpv.set_property("loop-file", "inf")?; // never stop
        mpv.set_property("hwdec", "auto")?; // GPU decode when possible
        Ok(Self {
            mpv,
            render: None,
            path: path.to_string(),
        })
    }

    /// Create the GL render context
    pub fn start(&mut self, wl_display: *mut c_void) -> Result<(), Box<dyn std::error::Error>> {
        let render = self.mpv.create_render_context(vec![
            RenderParam::ApiType(RenderParamApiType::OpenGl),
            RenderParam::InitParams(OpenGLInitParams {
                get_proc_address,
                // Instance<Static> is ZST, so building a fresh owned one is free
                ctx: egl::Instance::new(egl::Static),
            }),
            RenderParam::WaylandDisplay(wl_display as *const c_void),
        ])?;

        self.render = Some(render);
        //TODO: use ref later
        self.mpv.command("loadfile", &[&self.path])?;
        Ok(())
    }

    pub fn is_started(&self) -> bool {
        self.render.is_some()
    }

    /// Draw the current video frame
    pub fn render(&self, width: i32, height: i32) {
        if let Some(render) = &self.render {
            // Reversed: flip for normal, no flip for flipped
            let flip = true;
            let _ = render.render::<()>(0, width, height, flip);
        }
    }
}
