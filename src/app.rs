use std::{cell::RefCell, fmt::Display, path::Path, sync::Arc};

use color_eyre::{eyre::ContextCompat, Result};
use glam::{vec3, Mat4, Vec2, Vec3};

use pollster::FutureExt;
use rand::Rng;
use wgpu::FilterMode;
use wgpu_profiler::{wgpu_profiler, GpuProfiler};
use winit::{dpi::PhysicalSize, window::Window};

use crate::{
    app::{
        instance::Instance,
        light::{AreaLight, Light},
    },
    camera::{CameraUniform, CameraUniformBinding},
    models::{self, GltfDocument},
    pass::{self, Pass},
    recorder::Recorder,
    utils::{
        self,
        world::{Read, World, Write},
        DrawIndexedIndirect, ImageDimentions, ResizableBuffer, ResizableBufferExt,
    },
    watcher::Watcher,
    Gpu,
};

pub mod bind_group_layout;
pub mod blitter;
pub mod gbuffer;
pub mod global_ubo;
pub mod instance;
pub mod light;
pub mod material;
pub mod mesh;
pub mod pipeline;
mod screenshot;
pub mod state;
pub mod texture;
mod view_target;

pub(crate) use view_target::ViewTarget;

use self::{
    gbuffer::GBuffer,
    instance::{InstanceId, InstancePool},
    light::LightPool,
    material::{MaterialId, MaterialPool},
    mesh::{MeshId, MeshPool, MeshRef},
    pipeline::PipelineArena,
    screenshot::ScreenshotCtx,
    state::{AppState, StateAction},
    texture::TexturePool,
};

pub(crate) const DEFAULT_SAMPLER_DESC: wgpu::SamplerDescriptor<'static> = wgpu::SamplerDescriptor {
    label: Some("Gltf Default Sampler"),
    address_mode_u: wgpu::AddressMode::Repeat,
    address_mode_v: wgpu::AddressMode::Repeat,
    address_mode_w: wgpu::AddressMode::Repeat,
    mag_filter: FilterMode::Linear,
    min_filter: FilterMode::Linear,
    mipmap_filter: FilterMode::Linear,
    lod_min_clamp: 0.0,
    lod_max_clamp: std::f32::MAX,
    compare: None,
    anisotropy_clamp: 1,
    border_color: None,
};

pub struct App {
    pub gpu: Arc<Gpu>,
    pub surface: wgpu::Surface,
    pub surface_config: wgpu::SurfaceConfiguration,
    gbuffer: GBuffer,
    view_target: view_target::ViewTarget,

    global_uniform: global_ubo::Uniform,

    pub world: World,

    draw_cmd_buffer: ResizableBuffer<DrawIndexedIndirect>,
    draw_cmd_bind_group: wgpu::BindGroup,

    moving_instances: ResizableBuffer<InstanceId>,
    moving_instances_bind_group: wgpu::BindGroup,

    visibility_pass: pass::visibility::Visibility,
    emit_draws_pass: pass::visibility::EmitDraws,

    shading_pass: pass::shading::ShadingPass,

    postprocess_pass: pass::postprocess::PostProcess,

    update_pass: pass::compute_update::ComputeUpdate,

    taa_pass: pass::taa::Taa,

    default_sampler: wgpu::Sampler,

    pub blitter: blitter::Blitter,

    recorder: Recorder,
    screenshot_ctx: ScreenshotCtx,
    profiler: RefCell<wgpu_profiler::GpuProfiler>,
}

impl App {
    pub const SAMPLE_COUNT: u32 = 1;

    pub fn new(window: &Window, file_watcher: Watcher) -> Result<Self> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::VULKAN,
            dx12_shader_compiler: wgpu::Dx12Compiler::Fxc,
        });

        let surface = unsafe { instance.create_surface(&window) }?;

        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                force_fallback_adapter: false,
                compatible_surface: Some(&surface),
            })
            .block_on()
            .context("Failed to create Adapter")?;

        let limits = adapter.limits();
        let mut features = adapter.features();
        features.remove(wgpu::Features::MAPPABLE_PRIMARY_BUFFERS);

        let (device, queue) = adapter
            .request_device(
                &wgpu::DeviceDescriptor {
                    label: Some("Device"),
                    features,
                    limits,
                },
                None,
            )
            .block_on()?;
        let gpu = Arc::new(Gpu {
            device,
            queue,
            adapter,
        });

        let PhysicalSize { width, height } = window.inner_size();
        let surface_config = surface
            .get_default_config(gpu.adapter(), width, height)
            .context("Surface in not supported")?;
        surface.configure(gpu.device(), &surface_config);
        let gbuffer = GBuffer::new(&gpu, surface_config.width, surface_config.height);

        let mut world = World::new(gpu.clone());
        world.insert(PipelineArena::new(gpu.clone(), file_watcher));

        let view_target = view_target::ViewTarget::new(&world, width, height);

        let global_uniform = global_ubo::Uniform {
            resolution: [surface_config.width as f32, surface_config.height as f32],
            ..Default::default()
        };

        let default_sampler = gpu.device().create_sampler(&DEFAULT_SAMPLER_DESC);

        let draw_cmd_buffer = ResizableBuffer::new(
            gpu.device(),
            wgpu::BufferUsages::INDIRECT | wgpu::BufferUsages::STORAGE,
        );
        let draw_cmd_bind_group = draw_cmd_buffer.create_storage_write_bind_group(&mut world);

        let path = Path::new("shaders").join("postprocess.wgsl");
        let postprocess_pass = pass::postprocess::PostProcess::new(&mut world, path)?;

        let visibility_pass = pass::visibility::Visibility::new(&world)?;
        let emit_draws_pass = pass::visibility::EmitDraws::new(&world)?;

        let shading_pass = pass::shading::ShadingPass::new(&world, &gbuffer)?;

        let profiler = RefCell::new(GpuProfiler::new(
            4,
            gpu.queue().get_timestamp_period(),
            features,
        ));

        let moving_instances = gpu
            .device()
            .create_resizable_buffer(wgpu::BufferUsages::STORAGE);
        let moving_instances_bind_group =
            moving_instances.create_storage_read_bind_group(&mut world);

        let path = Path::new("shaders").join("compute_update.wgsl");
        let update_pass = pass::compute_update::ComputeUpdate::new(&world, path)?;

        let taa_pass = pass::taa::Taa::new(&world, &gbuffer, width, height)?;

        Ok(Self {
            surface,
            surface_config,
            gbuffer,
            view_target,

            default_sampler,

            global_uniform,

            postprocess_pass,

            draw_cmd_buffer,
            draw_cmd_bind_group,

            moving_instances,
            moving_instances_bind_group,

            visibility_pass,
            emit_draws_pass,

            shading_pass,

            update_pass,

            taa_pass,

            profiler,
            blitter: blitter::Blitter::new(&world),
            screenshot_ctx: ScreenshotCtx::new(&gpu, width, height),
            recorder: Recorder::new(),

            world,
            gpu,
        })
    }

    fn add_area_light(
        &mut self,
        color: Vec3,
        intensity: f32,
        wh: Vec2,
        transform: Mat4,
    ) -> Result<()> {
        self.world
            .get_mut::<LightPool>()?
            .add_area_light(&[AreaLight::from_transform(color, intensity, wh, transform)]);
        self.get_instance_pool_mut().add(&[Instance::new(
            transform * Mat4::from_scale((wh / 2.).extend(1.)),
            mesh::MeshPool::PLANE_MESH,
            MaterialPool::LIGHT_MATERIAL,
        )]);
        Ok(())
    }

    pub fn setup_scene(&mut self) -> Result<()> {
        use std::f32::consts::PI;
        let now = std::time::Instant::now();
        let mut instances = vec![];

        self.world
            .get_mut::<LightPool>()?
            .add_point_light(&[Light::new(vec3(0., 0.5, 0.), 10., vec3(1., 1., 1.))]);

        self.add_area_light(
            vec3(1., 1., 1.),
            7.,
            (5., 8.).into(),
            Mat4::from_translation(vec3(0., 10., 15.)) * Mat4::from_rotation_x(-PI / 4.),
        )?;
        self.add_area_light(
            vec3(1., 1., 1.),
            7.,
            (5., 8.).into(),
            Mat4::from_translation(vec3(0., 10., -25.)) * Mat4::from_rotation_x(-3. * PI / 4.),
        )?;

        let gltf_scene = GltfDocument::import(
            self,
            "assets/glTF-Sample-Models/2.0/Sponza/glTF/Sponza.gltf",
            // "assets/glTF-Sample-Models/2.0/AntiqueCamera/glTF/AntiqueCamera.gltf",
            // "assets/glTF-Sample-Models/2.0/Buggy/glTF-Binary/Buggy.glb",
            // "assets/glTF-Sample-Models/2.0/FlightHelmet/glTF/FlightHelmet.gltf",
            // "assets/glTF-Sample-Models/2.0/DamagedHelmet/glTF-Binary/DamagedHelmet.glb",
        )?;

        instances.extend(gltf_scene.get_scene_instances(
            Mat4::from_rotation_y(PI / 2.)
                * Mat4::from_translation(vec3(7., -5., 1.))
                * Mat4::from_scale(Vec3::splat(3.)),
        ));

        let helmet = GltfDocument::import(
            self,
            "assets/glTF-Sample-Models/2.0/DamagedHelmet/glTF-Binary/DamagedHelmet.glb",
        )?;
        instances.extend(helmet.get_scene_instances(
            Mat4::from_translation(vec3(0., 0., 9.)) * Mat4::from_scale(Vec3::splat(3.)),
        ));

        let gltf_ferris = GltfDocument::import(self, "assets/ferris3d_v1.0.glb")?;
        instances.extend(gltf_ferris.get_scene_instances(
            Mat4::from_translation(vec3(-3., -5.0, -4.)) * Mat4::from_scale(Vec3::splat(3.)),
        ));
        instances.extend(gltf_ferris.get_scene_instances(
            Mat4::from_translation(vec3(2., -5.0, -2.)) * Mat4::from_scale(Vec3::splat(3.)),
        ));
        gltf_ferris.get_scene_instances(
            Mat4::from_translation(vec3(2., -5.0, -2.)) * Mat4::from_scale(Vec3::splat(3.)),
        );
        self.world.get_mut::<InstancePool>()?.add(&instances);

        let sphere_mesh = models::make_uv_sphere(1.0, 10);
        let sphere_mesh_id = self.get_mesh_pool_mut().add(sphere_mesh.as_ref());

        let mut moving_instances = vec![];
        let mut rng = rand::thread_rng();
        let num = 10;
        for i in 0..num {
            let r = 3.5;
            let angle = 2. * PI * (i as f32) / num as f32;
            let x = r * angle.cos();
            let y = r * angle.sin();

            moving_instances.push(instance::Instance::new(
                Mat4::from_translation(vec3(x, y, -17.)),
                sphere_mesh_id,
                MaterialId::new(rng.gen_range(0..self.get_material_pool().buffer.len() as u32)),
            ));

            moving_instances.extend(gltf_ferris.get_scene_instances(
                Mat4::from_translation(vec3(x, y + 0., -9.))
                    * Mat4::from_rotation_z(angle)
                    * Mat4::from_scale(Vec3::splat(2.5)),
            ));
        }

        let moving_instances_id = self.world.get_mut::<InstancePool>()?.add(&moving_instances);
        self.moving_instances.push(&self.gpu, &moving_instances_id);

        let mut encoder = self.device().create_command_encoder(&Default::default());
        self.draw_cmd_buffer.set_len(
            &self.gpu.device,
            &mut encoder,
            self.world.get_mut::<InstancePool>()?.count() as _,
        );

        self.draw_cmd_bind_group = self
            .draw_cmd_buffer
            .create_storage_write_bind_group(&mut self.world);
        self.moving_instances_bind_group = self
            .moving_instances
            .create_storage_read_bind_group(&mut self.world);

        println!("Scene complete: {:?}", now.elapsed());

        Ok(())
    }

    pub fn render(&self, _state: &AppState) -> Result<(), wgpu::SurfaceError> {
        let mut profiler = self.profiler.borrow_mut();
        let target = self.surface.get_current_texture()?;
        let target_view = target.texture.create_view(&Default::default());

        let mut encoder = self
            .device()
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Main Render Encoder"),
            });

        profiler.begin_scope("Main Render Scope ", &mut encoder, self.device());

        wgpu_profiler!("Visibility", profiler, &mut encoder, self.device(), {
            wgpu_profiler!("Emit Draws", profiler, &mut encoder, self.device(), {
                self.emit_draws_pass.record(
                    &self.world,
                    &mut encoder,
                    pass::visibility::EmitDrawsResource {
                        draw_cmd_bind_group: &self.draw_cmd_bind_group,
                        draw_cmd_buffer: &self.draw_cmd_buffer,
                    },
                );
            });

            wgpu_profiler!("Geometry", profiler, &mut encoder, self.device(), {
                self.visibility_pass.record(
                    &self.world,
                    &mut encoder,
                    pass::visibility::VisibilityResource {
                        gbuffer: &self.gbuffer,
                        draw_cmd_buffer: &self.draw_cmd_buffer,
                    },
                );
            });
        });

        wgpu_profiler!("Shading", profiler, &mut encoder, self.device(), {
            self.shading_pass.record(
                &self.world,
                &mut encoder,
                pass::shading::ShadingResource {
                    gbuffer: &self.gbuffer,
                    view_target: &self.view_target,
                },
            );
        });

        wgpu_profiler!("Taa", profiler, &mut encoder, self.device(), {
            self.taa_pass.record(
                &self.world,
                &mut encoder,
                pass::taa::TaaResource {
                    gbuffer: &self.gbuffer,
                    view_target: &self.view_target,
                    width_height: (self.surface_config.width, self.surface_config.height),
                },
            );
        });

        wgpu_profiler!("Postprocess", profiler, &mut encoder, self.device(), {
            self.postprocess_pass.record(
                &self.world,
                &mut encoder,
                pass::postprocess::PostProcessResource {
                    sampler: &self.default_sampler,
                    view_target: &self.view_target,
                },
            );
        });

        self.blitter.blit_to_texture_with_binding(
            &mut encoder,
            &self.world.device(),
            self.view_target.main_binding(),
            &target_view,
            self.surface_config.format,
        );

        profiler.end_scope(&mut encoder);
        profiler.resolve_queries(&mut encoder);

        self.gpu.queue().submit(Some(encoder.finish()));
        target.present();

        profiler.end_frame().ok();

        if self.recorder.is_active() {
            self.capture_frame(|frame, _| {
                self.recorder.record(frame);
            });
        }

        Ok(())
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        if self.surface_config.width == width && self.surface_config.height == height {
            return;
        }
        self.surface_config.width = width;
        self.surface_config.height = height;
        self.surface
            .configure(self.gpu.device(), &self.surface_config);
        self.gbuffer.resize(&self.gpu, width, height);
        self.view_target = view_target::ViewTarget::new(&self.world, width, height);
        self.global_uniform.resolution = [width as f32, height as f32];

        self.screenshot_ctx.resize(&self.gpu, width, height);
        self.taa_pass.resize(self.gpu.device(), width, height);

        if self.recorder.is_active() {
            self.recorder.finish();
        }
    }

    pub fn update(&mut self, state: &AppState, actions: Vec<StateAction>) -> Result<()> {
        self.global_uniform.frame = state.frame_count as _;
        self.global_uniform.time = state.total_time as _;
        self.world
            .get_mut::<global_ubo::GlobalUniformBinding>()?
            .update(self.gpu.queue(), &self.global_uniform);

        let jitter = self.taa_pass.get_jitter(
            state.frame_count as u32,
            self.surface_config.width,
            self.surface_config.height,
        );
        let mut camera_uniform = self.world.get_mut::<CameraUniform>()?;
        *camera_uniform = state
            .camera
            .get_uniform(Some(jitter.to_array()), Some(&camera_uniform));
        self.world
            .get_mut::<CameraUniformBinding>()?
            .update(self.gpu.queue(), &camera_uniform);

        let mut encoder =
            self.gpu
                .device()
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("Compute Update"),
                });
        let resources = pass::compute_update::ComputeUpdateResourse {
            idx_bind_group: &self.moving_instances_bind_group,
            dispatch_size: self.moving_instances.len() as u32,
        };
        self.update_pass
            .record(&self.world, &mut encoder, resources);
        self.gpu.queue().submit(Some(encoder.finish()));

        if state.frame_count % 500 == 0 && std::env::var("GPU_PROFILING").is_ok() {
            let mut last_profile = vec![];
            while let Some(profiling_data) = self.profiler.borrow_mut().process_finished_frame() {
                last_profile = profiling_data;
            }
            utils::scopes_to_console_recursive(&last_profile, 0);
            println!();
        }

        for action in actions {
            match action {
                StateAction::Screenshot => {
                    self.capture_frame(|frame, dims| {
                        self.recorder.screenshot(frame, dims);
                    });
                }
                StateAction::StartRecording => {
                    self.recorder.start(self.screenshot_ctx.image_dimentions)
                }
                StateAction::FinishRecording => self.recorder.finish(),
            }
        }
        Ok(())
    }

    pub fn handle_events(&mut self, path: std::path::PathBuf) {
        self.world
            .unwrap_mut::<PipelineArena>()
            .reload_pipelines(&path);
    }

    pub fn capture_frame(&self, callback: impl FnOnce(Vec<u8>, ImageDimentions)) {
        let (frame, dims) = self.screenshot_ctx.capture_frame(
            &self.world,
            &self.blitter,
            self.view_target.main_view(),
        );
        callback(frame, dims)
    }

    pub fn add_mesh(&mut self, mesh: MeshRef) -> MeshId {
        self.world.get_mut::<MeshPool>().unwrap().add(mesh)
    }

    pub fn get_material_pool(&self) -> Read<MaterialPool> {
        self.world.get::<MaterialPool>().unwrap()
    }

    pub fn get_material_pool_mut(&self) -> Write<MaterialPool> {
        self.world.get_mut::<MaterialPool>().unwrap()
    }

    pub fn get_texture_pool(&self) -> Read<TexturePool> {
        self.world.get::<TexturePool>().unwrap()
    }

    pub fn get_texture_pool_mut(&self) -> Write<TexturePool> {
        self.world.get_mut::<TexturePool>().unwrap()
    }

    pub fn get_mesh_pool(&self) -> Read<MeshPool> {
        self.world.get::<MeshPool>().unwrap()
    }

    pub fn get_mesh_pool_mut(&self) -> Write<MeshPool> {
        self.world.get_mut::<MeshPool>().unwrap()
    }

    pub fn get_instance_pool(&self) -> Read<InstancePool> {
        self.world.get::<InstancePool>().unwrap()
    }

    pub fn get_instance_pool_mut(&self) -> Write<InstancePool> {
        self.world.get_mut::<InstancePool>().unwrap()
    }

    pub fn queue(&self) -> &wgpu::Queue {
        self.gpu.queue()
    }

    pub fn device(&self) -> &wgpu::Device {
        self.gpu.device()
    }

    pub fn get_info(&self) -> RendererInfo {
        let info = self.gpu.adapter().get_info();
        RendererInfo {
            device_name: info.name,
            device_type: self.get_device_type().to_string(),
            vendor_name: self.get_vendor_name().to_string(),
            backend: self.get_backend().to_string(),
        }
    }

    fn get_vendor_name(&self) -> &str {
        match self.gpu.adapter().get_info().vendor {
            0x1002 => "AMD",
            0x1010 => "ImgTec",
            0x10DE => "NVIDIA Corporation",
            0x13B5 => "ARM",
            0x5143 => "Qualcomm",
            0x8086 => "INTEL Corporation",
            _ => "Unknown vendor",
        }
    }

    fn get_backend(&self) -> &str {
        match self.gpu.adapter().get_info().backend {
            wgpu::Backend::Empty => "Empty",
            wgpu::Backend::Vulkan => "Vulkan",
            wgpu::Backend::Metal => "Metal",
            wgpu::Backend::Dx12 => "Dx12",
            wgpu::Backend::Dx11 => "Dx11",
            wgpu::Backend::Gl => "GL",
            wgpu::Backend::BrowserWebGpu => "Browser WGPU",
        }
    }

    fn get_device_type(&self) -> &str {
        match self.gpu.adapter().get_info().device_type {
            wgpu::DeviceType::Other => "Other",
            wgpu::DeviceType::IntegratedGpu => "Integrated GPU",
            wgpu::DeviceType::DiscreteGpu => "Discrete GPU",
            wgpu::DeviceType::VirtualGpu => "Virtual GPU",
            wgpu::DeviceType::Cpu => "CPU",
        }
    }
}

#[derive(Debug)]
pub struct RendererInfo {
    pub device_name: String,
    pub device_type: String,
    pub vendor_name: String,
    pub backend: String,
}

impl Display for RendererInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "Vendor name: {}", self.vendor_name)?;
        writeln!(f, "Device name: {}", self.device_name)?;
        writeln!(f, "Device type: {}", self.device_type)?;
        writeln!(f, "Backend: {}", self.backend)?;
        Ok(())
    }
}
