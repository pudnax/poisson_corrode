use std::time::Duration;

use app::MeshInfo;
use bvh::{BvhNode, Tlas, TlasNode};
use eyre::Result;
use voidin::*;
use winit::window::WindowAttributes;

#[allow(dead_code)]
struct Demo {
    pipeline: RenderHandle,

    tlas: Tlas,
    tlas_nodes: ResizableBuffer<TlasNode>,

    geometry_bind_group: wgpu::BindGroup,
}

impl Example for Demo {
    fn name() -> &'static str {
        "Bvh GPU"
    }

    fn init(app: &mut App) -> Result<Self> {
        let geometry_bgl =
            app.device()
                .create_bind_group_layout_wrap(&wgpu::BindGroupLayoutDescriptor {
                    label: Some("Trace BGL"),
                    entries: &[
                        wgpu::BindGroupLayoutEntry {
                            binding: 0,
                            visibility: wgpu::ShaderStages::FRAGMENT,
                            ty: wgpu::BindingType::Buffer {
                                ty: wgpu::BufferBindingType::Storage { read_only: true },
                                has_dynamic_offset: false,
                                min_binding_size: Some(TlasNode::NSIZE),
                            },
                            count: None,
                        },
                        wgpu::BindGroupLayoutEntry {
                            binding: 1,
                            visibility: wgpu::ShaderStages::FRAGMENT,
                            ty: wgpu::BindingType::Buffer {
                                ty: wgpu::BufferBindingType::Storage { read_only: true },
                                has_dynamic_offset: false,
                                min_binding_size: Some(Instance::NSIZE),
                            },
                            count: None,
                        },
                        wgpu::BindGroupLayoutEntry {
                            binding: 2,
                            visibility: wgpu::ShaderStages::FRAGMENT,
                            ty: wgpu::BindingType::Buffer {
                                ty: wgpu::BufferBindingType::Storage { read_only: true },
                                has_dynamic_offset: false,
                                min_binding_size: Some(MeshInfo::NSIZE),
                            },
                            count: None,
                        },
                        wgpu::BindGroupLayoutEntry {
                            binding: 3,
                            visibility: wgpu::ShaderStages::FRAGMENT,
                            ty: wgpu::BindingType::Buffer {
                                ty: wgpu::BufferBindingType::Storage { read_only: true },
                                has_dynamic_offset: false,
                                min_binding_size: Some(BvhNode::NSIZE),
                            },
                            count: None,
                        },
                        wgpu::BindGroupLayoutEntry {
                            binding: 4,
                            visibility: wgpu::ShaderStages::FRAGMENT,
                            ty: wgpu::BindingType::Buffer {
                                ty: wgpu::BufferBindingType::Storage { read_only: true },
                                has_dynamic_offset: false,
                                min_binding_size: Some(f32::NSIZE),
                            },
                            count: None,
                        },
                        wgpu::BindGroupLayoutEntry {
                            binding: 5,
                            visibility: wgpu::ShaderStages::FRAGMENT,
                            ty: wgpu::BindingType::Buffer {
                                ty: wgpu::BufferBindingType::Storage { read_only: true },
                                has_dynamic_offset: false,
                                min_binding_size: Some(u32::NSIZE),
                            },
                            count: None,
                        },
                    ],
                });
        let pipeline = {
            let camera_binding = app.world.get::<CameraUniformBinding>()?;
            app.get_pipeline_arena_mut()
                .process_render_pipeline_from_path(
                    "src/bin/bvh_trace.wgsl",
                    pipeline::RenderPipelineDescriptor {
                        layout: vec![
                            camera_binding.bind_group_layout.clone(),
                            geometry_bgl.clone(),
                        ],
                        depth_stencil: None,
                        ..Default::default()
                    },
                )?
        };

        let mut instances = vec![];
        let dragon_mesh = models::ObjModel::import(app, "assets/dragon.obj")?;
        for (mesh, material) in dragon_mesh {
            instances.push(Instance::new(
                Mat4::from_rotation_y(std::f32::consts::PI / 2.)
                    * Mat4::from_translation(vec3(0., 2., 0.))
                    * Mat4::from_scale(Vec3::splat(10.)),
                mesh,
                material,
            ));
        }
        let bnuuy_mesh = models::ObjModel::import(app, "assets/bunny.obj")?;
        for [x, y] in [[8., 8.], [-8., 8.], [8., -8.], [-8., -8.]] {
            for (mesh, material) in &bnuuy_mesh {
                instances.push(Instance::new(
                    Mat4::from_translation(vec3(x, y, 0.)) * Mat4::from_scale(Vec3::splat(3.)),
                    *mesh,
                    *material,
                ));
            }
        }

        app.get_instance_pool_mut().add(&instances);
        let mut tlas = Tlas::empty();
        tlas.build(&instances, &app.get_mesh_pool().mesh_info_cpu);

        let tlas_nodes = app
            .device()
            .create_resizable_buffer_init(&tlas.nodes, wgpu::BufferUsages::STORAGE);

        let geometry_bind_group = app.device().create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Geometry Bind Group"),
            layout: &geometry_bgl,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: tlas_nodes.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: app.get_instance_pool().instances.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: app.get_mesh_pool().mesh_info.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: app.get_mesh_pool().bvh_nodes.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: app.get_mesh_pool().vertices.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 5,
                    resource: app.get_mesh_pool().indices.as_entire_binding(),
                },
            ],
        });

        Ok(Self {
            pipeline,
            tlas,
            tlas_nodes,
            geometry_bind_group,
        })
    }

    fn update(&mut self, _ctx: UpdateContext) {}

    fn resize(&mut self, _gpu: &Gpu, _width: u32, _height: u32) {}

    fn render(&mut self, mut ctx: RenderContext) {
        let camera = ctx.world.unwrap::<CameraUniformBinding>();
        let arena = ctx.world.unwrap::<PipelineArena>();
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
        pass.set_bind_group(0, &camera.binding, &[]);
        pass.set_bind_group(1, &self.geometry_bind_group, &[]);
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
    let window = WindowAttributes::default();

    let camera = Camera::new(vec3(0., 2.5, 15.), 0., 0.);
    run::<Demo>(window, camera)
}
