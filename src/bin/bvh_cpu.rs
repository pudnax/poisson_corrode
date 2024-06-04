use std::{array, time::Duration};

use bvh::{Bvh, Dist::*, Ray};

use eyre::Result;
use glam::Vec4Swizzles;
use half::f16;
use rand::Rng;
use voidin::*;
use winit::window::WindowAttributes;

const WIDTH: usize = 640;
const HEIGHT: usize = 640;
type Pixel = [f16; 4];
const PIXEL_SIZE: usize = std::mem::size_of::<Pixel>();

struct Demo {
    cpu_pixels: Vec<Pixel>,
    gpu_pixels: wgpu::Buffer,

    vertices: Vec<Vec3>,
    indices: Vec<UVec3>,

    bvh: Bvh,
}

impl Example for Demo {
    fn name() -> &'static str {
        "Bvh CPU"
    }

    fn init(app: &mut App) -> Result<Self> {
        let cpu_pixels = vec![[f16::ZERO; 4]; WIDTH * HEIGHT];
        let gpu_pixels = app.device().create_buffer(&wgpu::BufferDescriptor {
            label: Some("Pixels"),
            size: (WIDTH * HEIGHT * PIXEL_SIZE) as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let mut rng = rand::thread_rng();
        let n = 64;
        let mut vertices = Vec::with_capacity(n * 3);
        for _ in 0..n {
            let v0 = Vec3::from(array::from_fn(|_| rng.gen_range(0. ..1.)));
            let v1 = Vec3::from(array::from_fn(|_| rng.gen_range(0. ..1.)));
            let v2 = Vec3::from(array::from_fn(|_| rng.gen_range(0. ..1.)));
            let base = v0 * 9. - vec3(5., 5., 0.);
            vertices.push(base);
            vertices.push(base + v1);
            vertices.push(base + v2);
        }
        let indices: Vec<_> = (0..vertices.len() as u32).collect();
        let mut indices: Vec<_> = indices.chunks_exact(3).map(UVec3::from_slice).collect();

        let bvh = bvh::BvhBuilder::new(&vertices, &mut indices).build();

        Ok(Self {
            cpu_pixels,
            gpu_pixels,

            vertices,
            indices,

            bvh,
        })
    }

    fn update(&mut self, _ctx: UpdateContext) {}

    fn resize(&mut self, _gpu: &Gpu, _width: u32, _height: u32) {}

    fn render(&mut self, mut ctx: RenderContext) {
        let camera = ctx.app_state.camera.get_uniform(None);
        for (i, p) in self.cpu_pixels.iter_mut().enumerate() {
            let x = (i % WIDTH) as f32 / WIDTH as f32;
            let y = (i / HEIGHT) as f32 / HEIGHT as f32;
            let Vec2 { x, y } = (vec2(x, y) - 0.5) * vec2(2., -2.);

            let view_pos = camera.clip_to_world * vec4(x, y, 1., 1.);
            let view_tang = camera.clip_to_world * vec4(x, y, 0., 1.);

            let eye = view_pos.xyz() / view_pos.w;
            let dir = view_tang.xyz().normalize();

            let ray = Ray::new(eye, dir);

            // let hit = self.bvh.traverse(&self.vertices, &self.indices, ray, 0, 1e30);
            let hit = self.bvh.traverse_iter(&self.vertices, &self.indices, ray);
            let val = match hit {
                Hit(dist) => {
                    let limit = 50.;
                    f16::from_f32((limit - dist) / limit)
                }
                Miss => f16::ZERO,
            };
            *p = [val, val, val, f16::ONE];
        }

        ctx.gpu
            .queue()
            .write_buffer(&self.gpu_pixels, 0, bytemuck::cast_slice(&self.cpu_pixels));
        ctx.encoder.copy_buffer_to_texture(
            wgpu::ImageCopyBuffer {
                buffer: &self.gpu_pixels,
                layout: wgpu::ImageDataLayout {
                    offset: 0,
                    bytes_per_row: Some((WIDTH * PIXEL_SIZE) as _),
                    rows_per_image: None,
                },
            },
            ctx.view_target.main_texture().as_image_copy(),
            wgpu::Extent3d {
                width: WIDTH as _,
                height: HEIGHT as _,
                depth_or_array_layers: 1,
            },
        );

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
    let window = WindowAttributes::default()
        .with_inner_size(LogicalSize::new(WIDTH as u32, HEIGHT as u32))
        .with_resizable(false);

    let camera = Camera::new(vec3(0., 0., 15.), 0., 0.);
    run::<Demo>(window, camera)
}
