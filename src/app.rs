use std::ffi::c_void;

use log::{debug, info};
use smithay_client_toolkit::reexports::client::{
    Connection, Dispatch, Proxy, QueueHandle,
    globals::GlobalList,
    protocol::{wl_output, wl_surface},
};
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState},
    delegate_compositor, delegate_layer, delegate_output, delegate_registry,
    output::{OutputHandler, OutputState},
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    shell::{
        WaylandSurface,
        wlr_layer::{Anchor, LayerShell, LayerShellHandler, LayerSurface, LayerSurfaceConfigure},
    },
};
use wayland_protocols::wp::viewporter::client::{
    wp_viewport::WpViewport, wp_viewporter::WpViewporter,
};

use crate::config::{BackendKind, Config, parse_layer};
use crate::egl::{Egl, EglWindow};
use crate::player::Player;
use crate::render::{Pattern, Renderer};
use crate::{
    APP_NAME,
    backend::{Backend, BackendCtx},
};

pub struct App {
    conn: Connection,
    registry_state: RegistryState,
    output_state: OutputState,
    layer: LayerSurface,

    viewport: WpViewport, // Not sure
    output: Option<wl_output::WlOutput>,
    egl: Egl,
    egl_window: Option<EglWindow>,
    /// The active frame source, chosen from config
    backend: Backend,
    /// Logical width
    width: u32,
    /// Logical height
    height: u32,
    /// Physical width
    phys_w: u32, //maybe i32
    /// Physical height
    phys_h: u32,
    first_configure: bool,
    exit: bool,
}

impl App {
    pub fn new(
        globals: &GlobalList,
        qh: &QueueHandle<Self>,
        conn: &Connection,
        video_path: &str,
        config: &Config,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        // Only the requested backend is constructed
        let backend = match config.backend {
            BackendKind::Mpv => Backend::Mpv(Player::new(
                video_path,
                &config.player,
                config.debug.enabled,
            )?),
            BackendKind::Pattern => Backend::Glow(Renderer::new(Pattern::Checkerboard)),
        };

        let compositor = CompositorState::bind(globals, qh)?;
        let layer_shell = LayerShell::bind(globals, qh)?;

        let surface = compositor.create_surface(qh);
        let layer = layer_shell.create_layer_surface(
            qh,
            surface,
            parse_layer(&config.layer.layer),
            Some(APP_NAME),
            None,
        );

        // Full screen
        layer.set_anchor(Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT);
        layer.set_size(0, 0);
        // Under other panels by default; configurable via layer.exclusive_zone
        layer.set_exclusive_zone(config.layer.exclusive_zone);

        // A viewport for physical-resolution
        let viewporter: WpViewporter = globals.bind(qh, 1..=1, ())?;
        let viewport = viewporter.get_viewport(layer.wl_surface(), qh, ());

        layer.commit();

        let display_ptr = conn.backend().display_ptr() as *mut c_void;
        let egl = Egl::new(display_ptr)?;

        Ok(Self {
            conn: conn.clone(),
            registry_state: RegistryState::new(globals),
            output_state: OutputState::new(globals, qh),
            layer,
            viewport,
            output: None,
            egl,
            egl_window: None,
            backend,
            width: 0,
            height: 0,
            phys_w: 0,
            phys_h: 0,
            first_configure: true,
            exit: false,
        })
    }

    pub fn exit(&self) -> bool {
        //
        self.exit
    }

    /// Get output's hardware resolution
    fn get_physical_size(&self) -> (u32, u32) {
        if let Some(output) = &self.output
            && let Some(info) = self.output_state.info(output)
            && let Some(mode) = info.modes.iter().find(|m| m.current)
        {
            return (mode.dimensions.0 as u32, mode.dimensions.1 as u32);
        }

        // Else just default
        debug!("Using default width and height");
        (self.width, self.height)
    }

    /// Recompute the physical render and resize the EGL window
    fn apply_size(&mut self) {
        if self.width == 0 || self.height == 0 {
            return;
        }

        let (pw, ph) = self.get_physical_size();
        self.phys_w = pw;
        self.phys_h = ph;
        info!(
            "Transalating: logical {}x{} → physical {}x{}",
            self.width, self.height, self.phys_w, self.phys_h
        );

        // Set logical size
        self.viewport
            .set_destination(self.width as i32, self.height as i32);

        let (pw, ph) = (self.phys_w as i32, self.phys_h as i32);

        if let Some(e) = self.egl_window.as_ref() {
            e.resize(pw, ph);
        } else {
            let window = self
                .egl
                .create_window(self.layer.wl_surface(), pw, ph)
                .expect("create egl window");

            let display_ptr = self.conn.backend().display_ptr() as *mut c_void;
            self.backend
                .init(BackendCtx {
                    gl: &window.gl,
                    display_ptr,
                })
                .expect("init backend");

            self.egl_window = Some(window);
        }
    }

    fn draw(&mut self, qh: &QueueHandle<Self>, time: u32) {
        debug_assert!(
            self.egl_window.is_some(),
            "If this panics on your setup, please create an issue listing your specs"
        );
        // The window may not exist, skip until it does
        let Some(window) = &self.egl_window else {
            return;
        };

        self.egl.bind(window).expect("make current");

        self.backend
            .render(&window.gl, self.phys_w as i32, self.phys_h as i32, time);

        // Schedule the next frame
        let surface = self.layer.wl_surface();
        surface.frame(qh, surface.clone());

        // Write to socket
        self.conn.flush().ok();
        // Present new frame
        self.egl.swap_buffers(window).expect("swap buffers");
    }
}

impl CompositorHandler for App {
    fn frame(
        &mut self,
        _conn: &Connection,
        qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        time: u32,
    ) {
        self.draw(qh, time);
    }

    fn surface_enter(
        &mut self,
        _c: &Connection,
        _q: &QueueHandle<Self>,
        _s: &wl_surface::WlSurface,
        output: &wl_output::WlOutput,
    ) {
        self.output = Some(output.clone());
        self.apply_size();
    }

    fn scale_factor_changed(
        &mut self,
        _c: &Connection,
        _q: &QueueHandle<Self>,
        _s: &wl_surface::WlSurface,
        _new: i32,
    ) {
    }
    fn transform_changed(
        &mut self,
        _c: &Connection,
        _q: &QueueHandle<Self>,
        _s: &wl_surface::WlSurface,
        _new: wl_output::Transform,
    ) {
        // No flips for now
    }
    fn surface_leave(
        &mut self,
        _c: &Connection,
        _q: &QueueHandle<Self>,
        _s: &wl_surface::WlSurface,
        _o: &wl_output::WlOutput,
    ) {
    }
}

impl LayerShellHandler for App {
    fn closed(&mut self, _c: &Connection, _q: &QueueHandle<Self>, _layer: &LayerSurface) {
        self.exit = true;
    }

    fn configure(
        &mut self,
        _conn: &Connection,
        qh: &QueueHandle<Self>,
        _layer: &LayerSurface,
        configure: LayerSurfaceConfigure,
        _serial: u32,
    ) {
        let (w, h) = configure.new_size;
        if w != 0 && h != 0 {
            self.width = w;
            self.height = h;
            self.apply_size();
        }

        if self.first_configure {
            self.first_configure = false;
            self.draw(qh, 0);
        }
    }
}

impl OutputHandler for App {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }
    fn new_output(&mut self, _c: &Connection, _q: &QueueHandle<Self>, _o: wl_output::WlOutput) {}
    fn update_output(&mut self, _c: &Connection, _q: &QueueHandle<Self>, _o: wl_output::WlOutput) {
        // Mode/resolution may have just become known or changed

        //TODO: later re-check output handling
        if self.output.is_some() {
            self.apply_size();
        }
    }
    fn output_destroyed(
        &mut self,
        _c: &Connection,
        _q: &QueueHandle<Self>,
        _o: wl_output::WlOutput,
    ) {
    }
}

impl ProvidesRegistryState for App {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }
    registry_handlers![OutputState];
}

impl Dispatch<WpViewporter, ()> for App {
    fn event(
        _: &mut Self,
        _: &WpViewporter,
        _: <WpViewporter as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<WpViewport, ()> for App {
    fn event(
        _: &mut Self,
        _: &WpViewport,
        _: <WpViewport as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

delegate_compositor!(App);
delegate_output!(App);
delegate_layer!(App);
delegate_registry!(App);
