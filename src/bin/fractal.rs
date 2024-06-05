
use app::GlobalsBindGroup;
use eyre::Result;
use voidin::*;

struct Triangle {
    pipeline: RenderHandle,
}

impl Example for Triangle {
    fn name() -> &'static str {
        "Triangle"
    }

    fn init(app: &mut App) -> Result<Self> {
        let camera = app.world.get::<CameraUniformBinding>()?;
        let globals = app.world.get::<GlobalsBindGroup>()?;
        let pipeline = app
            .get_pipeline_arena_mut()
            .process_render_pipeline_from_path(
                "src/bin/fractal.wgsl",
                pipeline::RenderPipelineDescriptor {
                    layout: vec![globals.layout.clone(), camera.bind_group_layout.clone()],
                    vertex: VertexState {
                        entry_point: "vs_main_trig".into(),
                        ..Default::default()
                    },
                    depth_stencil: None,
                    ..Default::default()
                },
            )?;
        Ok(Self { pipeline })
    }

    fn update(&mut self, _ctx: UpdateContext) {}

    fn resize(&mut self, _gpu: &Gpu, _width: u32, _height: u32) {}

    fn render(&mut self, mut ctx: RenderContext) {
        let arena = ctx.world.unwrap::<PipelineArena>();
        let globals = ctx.world.unwrap::<GlobalsBindGroup>();
        let camera = ctx.world.unwrap::<CameraUniformBinding>();
        let mut pass = ctx.encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: None,
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: ctx.view_target.main_view(),
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color {
                        r: 0.13,
                        g: 0.13,
                        b: 0.13,
                        a: 1.0,
                    }),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });

        pass.set_pipeline(arena.get_pipeline(self.pipeline));
        pass.set_bind_group(0, &globals.binding, &[]);
        pass.set_bind_group(1, &camera.binding, &[]);
        pass.draw(0..3, 0..1);
        drop(pass);

        // ctx.ui(|egui_ctx| {
        //     egui::Window::new("debug").show(egui_ctx, |ui| {
        //         ui.label(format!(
        //             "Fps: {:.04?}",
        //             Duration::from_secs_f64(ctx.app_state.dt)
        //         ));
        //     });
        // });
    }
}

fn main() -> Result<()> {
    run_default::<Triangle>()
}
