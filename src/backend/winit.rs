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
use std::time::Duration;

#[derive(Debug)]
pub struct WinitState {
    pub backend: WinitGraphicsBackend<GlowRenderer>,
    pub output: Output,
    pub damage_tracker: OutputDamageTracker,
    pub shaders: crate::render::Shaders,
}

impl WinitState {
    pub fn render(&mut self, state: &mut State) -> Result<(), Box<dyn std::error::Error>> {
        let age = self.backend.buffer_age().unwrap_or(0);
        let (renderer, mut fb) = self.backend.bind()?;

        let windows: Vec<_> = state.mon().visible_windows().collect();

        let result = crate::render::render_output(
            renderer,
            &mut fb,
            &mut self.damage_tracker,
            age,
            windows,
            &self.output,
            &self.shaders,
        )?;

        std::mem::drop(fb);
        self.backend.submit(result.damage.map(|x| x.as_slice()))?;

        let elapsed = state.start_time.elapsed();
        let output = self.output.clone();

        // frame callbacks for windows
        // TODO: convenience method in Monitor?
        for we in state.mon().visible_windows() {
            we.window
                .send_frame(&output, elapsed, Some(Duration::ZERO), |_, _| {
                    Some(output.clone())
                });
        }

        // frame callbacks for layer surfaces
        // TODO: convenience method in Monitor?
        let mut map = layer_map_for_output(&output);
        for layer in map.layers() {
            layer.send_frame(&output, elapsed, Some(Duration::ZERO), |_, _| {
                Some(output.clone())
            });
        }

        state.popups.cleanup();
        map.cleanup();
        drop(map);

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
                    monotile.state.mon_mut().recompute_layout();
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
