use log::{debug, warn};
use smithay_client_toolkit::reexports::client::{
    Connection, Dispatch, Proxy, QueueHandle, WEnum, event_created_child,
    protocol::{wl_output, wl_surface},
};
use smithay_client_toolkit::{
    compositor::CompositorHandler,
    delegate_compositor, delegate_layer, delegate_output, delegate_registry,
    output::{OutputHandler, OutputState},
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    shell::wlr_layer::{LayerShellHandler, LayerSurface, LayerSurfaceConfigure},
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

use super::{App, ToplevelState};

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
        self.status_dirty = true;
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
