use std::collections::{HashMap, HashSet};
use std::ffi::c_void;

use log::{debug, error, info, warn};
use smithay_client_toolkit::reexports::client::{
    Connection, Dispatch, Proxy, QueueHandle, WEnum,
    backend::ObjectId,
    event_created_child,
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
use wayland_protocols_wlr::foreign_toplevel::v1::client::{
    zwlr_foreign_toplevel_handle_v1::{Event as ToplevelEvent, ZwlrForeignToplevelHandleV1},
    zwlr_foreign_toplevel_manager_v1::{
        EVT_TOPLEVEL_OPCODE, Event as ManagerEvent, ZwlrForeignToplevelManagerV1,
    },
};
use wayland_protocols_wlr::output_power_management::v1::client::{
    zwlr_output_power_manager_v1::ZwlrOutputPowerManagerV1,
    zwlr_output_power_v1::{Event as PowerEvent, Mode as PowerMode, ZwlrOutputPowerV1},
};

use crate::config::{BackendKind, Config, parse_layer};
use crate::egl::{Egl, EglWindow};
use crate::player::Player;
use crate::render::{Pattern, Renderer};
use crate::{
    APP_NAME,
    backend::{Backend, BackendCtx},
};

#[derive(Default)]
struct ToplevelState {
    fullscreen: bool,
    maximized: bool,
    /// Window is active, needed for per-workspace handling
    activated: bool,
    /// Every output this toplevel currently reports being visible on
    outputs: HashSet<ObjectId>,
}

impl ToplevelState {
    /// Is the wallpaper hidden?
    fn to_hide(
        &self,
        our_output: Option<&ObjectId>,
        on_fullscreen: bool,
        on_maximized: bool,
    ) -> bool {
        let fullscreen_or_maximized =
            (on_fullscreen && self.fullscreen) || (on_maximized && self.maximized);
        let output_visible = our_output.is_some_and(|o| self.outputs.contains(o));

        output_visible && self.activated && fullscreen_or_maximized
    }
}

// TODO: maybe I should group these...
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
    /// True if some other window is covering the wallpaper (not fullproof)
    hidden: bool,
    /// Output switched off
    screen_off: bool,
    /// True while GameMode has at least one registered game
    gamemode_active: bool,
    /// Global for creating per-output power objects, if the compositor supports it
    power_manager: Option<ZwlrOutputPowerManagerV1>,
    /// DPMS power object for the current output
    power: Option<ZwlrOutputPowerV1>,
    /// Global for toplevel windows, if compositor has support
    toplevel_manager: Option<ZwlrForeignToplevelManagerV1>,
    /// State of every currently open toplevel panels
    toplevels: HashMap<ObjectId, ToplevelState>,
    /// Pause when a toplevel is fullscreen
    pause_on_fullscreen: bool,
    /// Pause when a toplevel is maximized (not fullscreen)
    pause_on_maximized: bool,
    /// True while a wl_surface.frame callback is outstanding
    frame_scheduled: bool,
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
        // Enumerates existing windows for top, must be before toplevel_manager
        let output_state = OutputState::new(globals, qh);

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

        // Neither of these wlr-only protocols is guaranteed to exist
        let power_manager: Option<ZwlrOutputPowerManagerV1> = if config.pausing.on_screen_off {
            // Protocol reference:
            // https://gitlab.freedesktop.org/wlroots/wlr-protocols/-/blob/master/unstable/wlr-output-power-management-unstable-v1.xml?ref_type=heads
            match globals.bind(qh, 1..=1, ()) {
                Ok(manager) => Some(manager),
                Err(err) => {
                    error!(
                        "Failed to bind wlr-output-power-management-v1; \
                        `on_screen_off` pausing is disabled: {err}"
                    );
                    None
                }
            }
        } else {
            None
        };

        let toplevel_manager: Option<ZwlrForeignToplevelManagerV1> =
            if config.pausing.on_fullscreen || config.pausing.on_maximized {
                // Protocol reference:
                // https://gitlab.freedesktop.org/wlroots/wlr-protocols/-/blob/master/unstable/wlr-foreign-toplevel-management-unstable-v1.xml?ref_type=heads
                match globals.bind(qh, 2..=3, ()) {
                    Ok(manager) => Some(manager),
                    Err(err) => {
                        log::error!(
                            "Failed to bind wlr-foreign-toplevel-management-v1; \
                             fullscreen/maximized-window pausing disabled: {err}"
                        );
                        None
                    }
                }
            } else {
                None
            };

        layer.commit();

        let display_ptr = conn.backend().display_ptr() as *mut c_void;
        let egl = Egl::new(display_ptr)?;

        Ok(Self {
            conn: conn.clone(),
            registry_state: RegistryState::new(globals),
            output_state,
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
            hidden: false,
            screen_off: false,
            gamemode_active: false,
            power_manager,
            power: None,
            toplevel_manager,
            toplevels: HashMap::new(),
            pause_on_fullscreen: config.pausing.on_fullscreen,
            pause_on_maximized: config.pausing.on_maximized,
            frame_scheduled: false,
        })
    }

    pub fn exit(&self) -> bool {
        //
        self.exit
    }

    /// True if backend should be paused
    fn should_pause(&self) -> bool {
        self.hidden || self.screen_off || self.gamemode_active
    }

    /// Call when obscured or hidden (assumed)
    fn set_hidden(&mut self, hidden: bool, qh: &QueueHandle<Self>) {
        // Skip if applied
        if self.hidden == hidden {
            return;
        }
        // Re-compute and re-apply
        self.hidden = hidden;
        self.apply_pause_edge(qh);
    }

    /// Call when screen is off
    fn set_screen_off(&mut self, screen_off: bool, qh: &QueueHandle<Self>) {
        // Skip if applied
        if self.screen_off == screen_off {
            return;
        }
        // Re-compute and re-apply
        self.screen_off = screen_off;
        self.apply_pause_edge(qh);
    }

    /// Call when gamemode is on
    pub fn set_gamemode(&mut self, active: bool, qh: &QueueHandle<Self>) {
        // Skip if applied
        if self.gamemode_active == active {
            return;
        }
        // Re-compute and re-apply
        self.gamemode_active = active;
        self.apply_pause_edge(qh);
    }

    /// Call after mutating `occluded`/`screen_off`; only touches the backend
    /// on the OR'd value's edge, to avoid redundant mpv calls
    pub fn apply_pause_edge(&mut self, qh: &QueueHandle<Self>) {
        let now_paused = self.should_pause();
        if now_paused {
            self.backend.pause();
        } else {
            self.backend.resume();
            // While paused, draw() stops re-arming the frame callback so without this black screen will happen
            if self.egl_window.is_some() && !self.frame_scheduled {
                self.draw(qh, 0);
            }
        }
    }

    /// Recompute `to_hide` from the current toplevel states
    fn recompute_hidden(&mut self, qh: &QueueHandle<Self>) {
        let our_output = self.output.as_ref().map(Proxy::id);
        let hidden = self.toplevels.values().any(|t| {
            t.to_hide(
                our_output.as_ref(),
                self.pause_on_fullscreen,
                self.pause_on_maximized,
            )
        });
        self.set_hidden(hidden, qh);
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

        let surface = self.layer.wl_surface();

        if self.should_pause() {
            // nothing to render.
            // apply_pause_edge() starts the loop back up on resume
            return;
        }

        self.egl.bind(window).expect("make current");

        self.backend
            .render(&window.gl, self.phys_w as i32, self.phys_h as i32, time);

        // Schedule the next frame
        surface.frame(qh, surface.clone());
        self.frame_scheduled = true;

        // Present new frame (like commit)
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
        self.frame_scheduled = false;
        self.draw(qh, time);
    }

    fn surface_enter(
        &mut self,
        _c: &Connection,
        qh: &QueueHandle<Self>,
        _s: &wl_surface::WlSurface,
        output: &wl_output::WlOutput,
    ) {
        let is_new_output = self.output.as_ref().map(Proxy::id) != Some(output.id());
        self.output = Some(output.clone());
        if is_new_output && let Some(manager) = &self.power_manager {
            // Rebind DPMS tracking to the output we're actually on now
            if let Some(old) = self.power.take() {
                old.destroy();
            }
            self.power = Some(manager.get_output_power(output, qh, ()));
        }
        // Toplevels may have reported their outputs before ours was known
        self.recompute_hidden(qh);
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

impl Dispatch<ZwlrOutputPowerManagerV1, ()> for App {
    fn event(
        _: &mut Self,
        _: &ZwlrOutputPowerManagerV1,
        _: <ZwlrOutputPowerManagerV1 as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ZwlrOutputPowerV1, ()> for App {
    fn event(
        app: &mut Self,
        proxy: &ZwlrOutputPowerV1,
        event: <ZwlrOutputPowerV1 as Proxy>::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        match event {
            PowerEvent::Mode {
                mode: WEnum::Value(PowerMode::On),
            } => app.set_screen_off(false, qh),
            PowerEvent::Mode {
                mode: WEnum::Value(PowerMode::Off),
            } => app.set_screen_off(true, qh),
            PowerEvent::Failed => {
                warn!("zwlr_output_power_v1 failed; falling back to occlusion detection only");
                proxy.destroy();
                app.power = None;
                // No more mode updates will come for this output, don't
                // leave playback stuck paused on a stale DPMS state
                app.set_screen_off(false, qh);
            }
            e => {
                warn!("Unknown power event: {e:?}");
            }
        }
    }
}

impl Dispatch<ZwlrForeignToplevelManagerV1, ()> for App {
    fn event(
        app: &mut Self,
        _proxy: &ZwlrForeignToplevelManagerV1,
        event: <ZwlrForeignToplevelManagerV1 as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            ManagerEvent::Toplevel { toplevel } => {
                app.toplevels
                    .insert(toplevel.id(), ToplevelState::default());
            }
            // Compositor is done with the manager
            ManagerEvent::Finished => app.toplevel_manager = None,
            _ => {}
        }
    }

    event_created_child!(App, ZwlrForeignToplevelManagerV1, [
        EVT_TOPLEVEL_OPCODE => (ZwlrForeignToplevelHandleV1, ()),
    ]);
}

impl Dispatch<ZwlrForeignToplevelHandleV1, ()> for App {
    fn event(
        app: &mut Self,
        proxy: &ZwlrForeignToplevelHandleV1,
        event: <ZwlrForeignToplevelHandleV1 as Proxy>::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        let id = proxy.id();
        // https://docs.rs/wayland-protocols-wlr/0.3.12/wayland_protocols_wlr/foreign_toplevel/v1/client/zwlr_foreign_toplevel_handle_v1/enum.Event.html
        match event {
            // The array is a sequence of u32 `state` enum values; it shows current state, not a delta!
            ToplevelEvent::State { state } => {
                if let Some(t) = app.toplevels.get_mut(&id) {
                    // Reset all state
                    t.fullscreen = false;
                    t.maximized = false;
                    t.activated = false;
                    for chunk in state.as_chunks::<4>().0 {
                        match u32::from_ne_bytes(*chunk) {
                            0 => t.maximized = true,
                            1 => {} // Minimized
                            2 => t.activated = true,
                            3 => t.fullscreen = true,
                            s => {
                                debug!("unknown toplevel state value: {s}");
                            }
                        }
                    }
                }
            }
            ToplevelEvent::OutputEnter { output } => {
                if let Some(t) = app.toplevels.get_mut(&id) {
                    t.outputs.insert(output.id());
                }
            }
            ToplevelEvent::OutputLeave { output } => {
                if let Some(t) = app.toplevels.get_mut(&id) {
                    t.outputs.remove(&output.id());
                }
            }
            // Marks the end of a batch of the events above; only now is the
            // toplevel's state consistent enough to act on
            ToplevelEvent::Done => app.recompute_hidden(qh),
            ToplevelEvent::Closed => {
                app.toplevels.remove(&id);
                proxy.destroy();
                app.recompute_hidden(qh);
            }
            _ => {}
        }
    }
}

delegate_compositor!(App);
delegate_output!(App);
delegate_layer!(App);
delegate_registry!(App);
