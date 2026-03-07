// SPDX-License-Identifier: GPL-3.0-only

use crate::{Monotile, state::State};
use smithay::{
    backend::{
        renderer::{damage::OutputDamageTracker, glow::GlowRenderer},
        winit::{self, WinitEvent, WinitGraphicsBackend},
    },
    desktop::layer_map_for_output,
    output::{Mode, Output, PhysicalProperties, Subpixel},
    reexports::calloop::EventLoop,
    utils::Transform,
};

#[derive(Debug)]
pub struct WinitState {
    pub backend: WinitGraphicsBackend<GlowRenderer>,
    pub output: Output,
    pub damage_tracker: OutputDamageTracker,
    pub shaders: crate::render::Shaders,
}

impl WinitState {
    pub fn render(&mut self, state: &mut State) -> Result<(), Box<dyn std::error::Error>> {
        // skip frame if a tiled window has a pending resize (no flicker)
        if state.windows.any_pending_resize(state.mon().tag()) {
            let mon = state.mon();
            crate::render::send_frame_callbacks(
                state.windows.visible(mon.tag()),
                &self.output,
                state.start_time.elapsed(),
                &mut state.popups,
            );
            self.backend.window().request_redraw();
            return Ok(());
        }

        let age = self.backend.buffer_age().unwrap_or(0);
        let (renderer, mut fb) = self.backend.bind()?;

        let elems = crate::render::output_elements(
            renderer,
            state.mon(),
            &state.windows,
            &self.shaders,
            &state.config,
        );
        let rendered = self.damage_tracker.render_output(
            renderer,
            &mut fb,
            age,
            &elems,
            state.config.colors.bg.0,
        )?;

        std::mem::drop(fb);
        self.backend.submit(rendered.damage.map(|x| x.as_slice()))?;

        let mon = &state.monitors[state.active_monitor];
        crate::render::send_frame_callbacks(
            state.windows.visible(mon.tag()),
            &self.output,
            state.start_time.elapsed(),
            &mut state.popups,
        );

        self.backend.window().request_redraw();
        Ok(())
    }
}

pub fn init(
    event_loop: &mut EventLoop<Monotile>,
    monotile: &mut Monotile,
) -> Result<(), Box<dyn std::error::Error>> {
    let (mut backend, winit) = winit::init()?;
    let shaders = crate::render::compile_shaders(backend.renderer());

    let mode = Mode {
        size: backend.window_size(),
        refresh: 60_000,
    };

    let output = Output::new(
        "winit".to_string(),
        PhysicalProperties {
            size: (0, 0).into(),
            subpixel: Subpixel::Unknown,
            make: "Smithay".into(),
            model: "Winit".into(),
            serial_number: "Unknown".into(),
        },
    );

    // the global id is not needed for winit
    let _global = output.create_global::<Monotile>(&monotile.state.display_handle);
    output.change_current_state(
        Some(mode),
        Some(Transform::Flipped180),
        None,
        Some((0, 0).into()),
    );
    output.set_preferred(mode);

    monotile.state.add_monitor(output.clone());

    let damage_tracker = OutputDamageTracker::from_output(&output);

    monotile.backend = crate::backend::Backend::Winit(WinitState {
        backend,
        output,
        damage_tracker,
        shaders,
    });

    event_loop
        .handle()
        .insert_source(winit, move |event, _, monotile| {
            match event {
                WinitEvent::Resized { size, .. } => {
                    monotile.backend.winit().output.change_current_state(
                        Some(Mode {
                            size,
                            refresh: 60_000,
                        }),
                        None,
                        None,
                        None,
                    );
                    layer_map_for_output(&monotile.backend.winit().output).arrange();
                    monotile.recompute_layout();
                }
                WinitEvent::Input(event) => monotile.process_input_event(event),
                WinitEvent::Redraw => {
                    let ws = monotile.backend.winit();
                    if let Err(err) = ws.render(&mut monotile.state) {
                        tracing::error!(?err, "Failed to render frame.");
                    }
                }
                WinitEvent::CloseRequested => {
                    monotile.state.loop_signal.stop();
                }
                _ => (),
            };
        })?;

    Ok(())
}
