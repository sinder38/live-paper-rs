use std::error::Error;
use std::ffi::c_void;

use khronos_egl as egl;
use smithay_client_toolkit::reexports::client::{Proxy, protocol::wl_surface::WlSurface};
use wayland_egl::WlEglSurface;

// The pixel format we ask EGL for:
// an on-screen (window) target,
// GLES2-capable,
// 8 bits each of R/G/B/A
const CONFIG_ATTRS: [egl::Int; 13] = [
    egl::SURFACE_TYPE,
    egl::WINDOW_BIT,
    egl::RENDERABLE_TYPE,
    egl::OPENGL_ES2_BIT,
    egl::RED_SIZE,
    8,
    egl::GREEN_SIZE,
    8,
    egl::BLUE_SIZE,
    8,
    egl::ALPHA_SIZE,
    8,
    egl::NONE,
];

// Ask for an OpenGL ES 3.x context
const CONTEXT_ATTRS: [egl::Int; 3] = [egl::CONTEXT_MAJOR_VERSION, 3, egl::NONE];

pub type EGLInstance = egl::Instance<egl::Static>;

/// A GPU render target for one Wayland surface
pub struct EglWindow {
    wl_egl: WlEglSurface,
    egl_surface: egl::Surface,
    pub gl: glow::Context,
}

impl EglWindow {
    /// Resize the underlying buffers (e.g. when the output geometry changes)
    pub fn resize(&self, width: i32, height: i32) {
        self.wl_egl.resize(width, height, 0, 0);
    }
}

/// The per-connection EGL state: display, chosen pixel format, and GL context
pub struct Egl {
    instance: EGLInstance,
    display: egl::Display,
    config: egl::Config,
    context: egl::Context,
}

impl Egl {
    pub fn new(wl_display_ptr: *mut c_void) -> Result<Self, Box<dyn Error>> {
        let instance = egl::Instance::new(egl::Static);

        let display =
            unsafe { instance.get_display(wl_display_ptr) }.ok_or("eglGetDisplay returned null")?;
        instance.initialize(display)?;
        // Tell EGL we want the OpenGL ES API (not desktop GL / OpenVG)
        instance.bind_api(egl::OPENGL_ES_API)?;

        let config = instance
            .choose_first_config(display, &CONFIG_ATTRS)?
            .ok_or("no matching EGL config")?;
        let context = instance.create_context(display, config, None, &CONTEXT_ATTRS)?;

        Ok(Self {
            instance,
            display,
            config,
            context,
        })
    }

    /// Wrap a `wl_surface` into a GPU render target of the given pixel size and
    /// load a GL function table bound to it
    pub fn create_window(
        &self,
        surface: &WlSurface,
        width: i32,
        height: i32,
    ) -> Result<EglWindow, Box<dyn Error>> {
        // wl_egl_window is the native-window handle EGL renders into; on swap it
        // attaches the GPU buffer to our wl_surface and commits
        let wl_egl = WlEglSurface::new(surface.id(), width, height)?;
        let egl_surface = unsafe {
            self.instance.create_window_surface(
                self.display,
                self.config,
                wl_egl.ptr() as egl::NativeWindowType,
                None,
            )?
        };

        // Bind context + surface to this thread so GL calls target it, then load
        // the GL functions via eglGetProcAddress

        self.make_current(egl_surface)?;
        let gl = unsafe {
            glow::Context::from_loader_function(|name| {
                self.instance
                    .get_proc_address(name)
                    .map_or(std::ptr::null(), |p| p as *const c_void)
            })
        };

        Ok(EglWindow {
            wl_egl,
            egl_surface,
            gl,
        })
    }

    /// Make the window current and present the frame just drawn
    pub fn bind(&self, window: &EglWindow) -> Result<(), Box<dyn Error>> {
        self.make_current(window.egl_surface)?;
        Ok(())
    }

    fn make_current(&self, surface: egl::Surface) -> Result<(), Box<dyn Error>> {
        self.instance.make_current(
            self.display,
            Some(surface),
            Some(surface),
            Some(self.context),
        )?;
        Ok(())
    }

    pub fn swap_buffers(&self, window: &EglWindow) -> Result<(), Box<dyn Error>> {
        self.instance
            .swap_buffers(self.display, window.egl_surface)?;
        Ok(())
    }
}
