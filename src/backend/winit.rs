// SPDX-License-Identifier: GPL-3.0-only

use tracing::info;

use crate::{Monotile, shell::MonitorSettings, state::State};
use smithay::{
    backend::{
        renderer::{damage::OutputDamageTracker, glow::GlowRenderer},
        winit::{self, WinitEvent, WinitGraphicsBackend},
    },
    desktop::{layer_map_for_output, utils::send_frames_surface_tree},
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
        // skip frame if a window has a pending resize (no flicker)
        let tag = state.monitors[state.active_monitor].tag();
        let throttle = Some(std::time::Duration::from_millis(16));
        if !state.locked && state.windows.any_pending_resize(tag) {
            crate::render::send_frame_callbacks(
                &mut state.windows,
                tag,
                &self.output,
                state.start_time.elapsed(),
                throttle,
                &mut state.popups,
            );
            self.backend.window().request_redraw();
            return Ok(());
        }

        let age = self.backend.buffer_age().unwrap_or(0);
        let (renderer, mut fb) = self.backend.bind()?;
        let mon = &state.monitors[state.active_monitor];
        let elems = crate::render::output_elements(
            renderer,
            mon,
            &mut state.windows,
            &self.shaders,
            &state.config,
            state.locked,
        );
        let rendered = self.damage_tracker.render_output(
            renderer,
            &mut fb,
            age,
            &elems,
            state.mon().settings.background,
        )?;

        std::mem::drop(fb);
        self.backend.submit(rendered.damage.map(|x| x.as_slice()))?;

        {
            let mon = &state.monitors[state.active_monitor];
            if let Some(ls) = &mon.lock_surface {
                send_frames_surface_tree(
                    ls.wl_surface(),
                    &self.output,
                    state.start_time.elapsed(),
                    None,
                    |_, _| Some(self.output.clone()),
                );
            }
        }
        let tag = state.monitors[state.active_monitor].tag();
        crate::render::send_frame_callbacks(
            &mut state.windows,
            tag,
            &self.output,
            state.start_time.elapsed(),
            throttle,
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

    output.change_current_state(
        Some(mode),
        Some(Transform::Flipped180),
        None,
        Some((0, 0).into()),
    );
    output.set_preferred(mode);

    monotile.state.add_monitor(output.clone(), MonitorSettings::default());
    info!("output: winit {}x{}", mode.size.w, mode.size.h);

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
