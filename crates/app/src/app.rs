use std::{fmt::Display, sync::Arc};

use eyre::{ContextCompat, Result};
// use egui_wgpu::ScreenDescriptor;
use glam::{Mat4, Vec2, Vec3};

use pollster::FutureExt;
use wgpu::{ComputePass, FilterMode, RenderPass};
// use wgpu_profiler::{GpuProfiler, GpuTimerScopeResult};
use winit::{dpi::PhysicalSize, window::Window};

use components::{
    bind_group_layout::{
        SingleTextureBindGroupLayout, StorageReadBindGroupLayout, StorageReadBindGroupLayoutDyn,
        StorageWriteBindGroupLayout, StorageWriteBindGroupLayoutDyn,
    },
    world::{Read, Write},
    Blitter, DrawIndexedIndirect, Gpu, ImageDimensions, RecordEvent, Recorder, ResizableBuffer,
    Watcher, World, {CameraUniform, CameraUniformBinding},
};

pub mod gbuffer;
pub mod global_ubo;
pub mod pipeline;
mod screenshot;
pub mod state;
mod view_target;

pub use view_target::ViewTarget;

use self::{
    gbuffer::GBuffer,
    global_ubo::GlobalsBindGroup,
    pipeline::PipelineArena,
    screenshot::ScreenshotCtx,
    state::{AppState, StateAction},
};
use crate::{
    AreaLight, Example, Instance, InstancePool, LightPool, MaterialPool, TexturePool,
    {MeshId, MeshPool, MeshRef},
};

pub const DEFAULT_SAMPLER_DESC: wgpu::SamplerDescriptor<'static> = wgpu::SamplerDescriptor {
    label: Some("Gltf Default Sampler"),
    address_mode_u: wgpu::AddressMode::Repeat,
    address_mode_v: wgpu::AddressMode::Repeat,
    address_mode_w: wgpu::AddressMode::Repeat,
    mag_filter: FilterMode::Linear,
    min_filter: FilterMode::Linear,
    mipmap_filter: FilterMode::Linear,
    lod_min_clamp: 0.0,
    lod_max_clamp: f32::MAX,
    compare: None,
    anisotropy_clamp: 1,
    border_color: None,
};

pub struct App {
    pub gpu: Arc<Gpu>,
    // pub surface: wgpu::Surface,
    pub surface_config: wgpu::SurfaceConfiguration,
    pub gbuffer: GBuffer,
    pub view_target: view_target::ViewTarget,

    global_uniform: global_ubo::Uniform,

    pub world: World,

    draw_cmd_buffer: ResizableBuffer<DrawIndexedIndirect>,
    draw_cmd_bind_group: wgpu::BindGroup,

    pub blitter: Blitter,

    recorder: Recorder,
    screenshot_ctx: ScreenshotCtx,
    // profiler: RefCell<wgpu_profiler::GpuProfiler>,
    // pub(crate) egui_context: egui::Context,
    // egui_renderer: egui_wgpu::Renderer,
    // pub(crate) egui_state: egui_winit::State,
}

impl App {
    pub const SAMPLE_COUNT: u32 = 1;

    // TODO: call resize right after
    pub fn new(
        instance: wgpu::Instance,
        window: &Window,
        surface: &wgpu::Surface<'_>,
        file_watcher: Watcher,
    ) -> Result<Self> {
        // let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        //     backends: wgpu::Backends::VULKAN,
        //     dx12_shader_compiler: wgpu::Dx12Compiler::Fxc,
        //     flags: InstanceFlags::default(),
        //     gles_minor_version: wgpu::Gles3MinorVersion::Automatic,
        // });

        // let surface = unsafe { instance.create_surface(&window) }?;

        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                force_fallback_adapter: false,
                compatible_surface: Some(surface),
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
                    required_features: features,
                    required_limits: limits,
                },
                None,
            )
            .block_on()?;
        let gpu = Arc::new(Gpu::new(adapter, device, queue));

        let PhysicalSize { width, height } = window.inner_size();
        let format = preferred_framebuffer_format(&surface.get_capabilities(gpu.adapter()).formats);
        let default_config = surface
            .get_default_config(gpu.adapter(), width, height)
            .context("Failed to create default surface config")?;
        let surface_config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            present_mode: wgpu::PresentMode::Fifo,
            ..default_config
        };
        surface.configure(gpu.device(), &surface_config);
        let gbuffer = GBuffer::new(&gpu, surface_config.width, surface_config.height);

        let mut world = {
            let mut world = World::new(gpu.clone());
            world.insert(PipelineArena::new(gpu.clone(), file_watcher));
            let camera = CameraUniformBinding::new(gpu.device());
            let globals = global_ubo::GlobalUniformBinding::new(gpu.device());
            world.insert(TexturePool::new(gpu.clone()));
            world.insert(MeshPool::new(gpu.clone()));
            world.insert(MaterialPool::new(gpu.clone()));
            world.insert(InstancePool::new(gpu.clone()));
            world.insert(LightPool::new(gpu.clone()));
            world.insert(GlobalsBindGroup::new(&gpu, &globals, &camera));
            world.insert(globals);
            world.insert(camera);
            world.insert(CameraUniform::default());
            world.insert(SingleTextureBindGroupLayout::new(&gpu));
            world.insert(StorageReadBindGroupLayoutDyn::new(&gpu));
            world.insert(StorageWriteBindGroupLayoutDyn::new(&gpu));
            world.insert(StorageReadBindGroupLayout::<u32>::new(&gpu));
            world.insert(StorageWriteBindGroupLayout::<u32>::new(&gpu));
            world.insert(StorageReadBindGroupLayout::<DrawIndexedIndirect>::new(&gpu));
            world.insert(StorageWriteBindGroupLayout::<DrawIndexedIndirect>::new(
                &gpu,
            ));
            world
        };

        let view_target = view_target::ViewTarget::new(&world, width, height);

        let global_uniform = global_ubo::Uniform {
            resolution: [surface_config.width as f32, surface_config.height as f32],
            ..Default::default()
        };

        let draw_cmd_buffer = ResizableBuffer::new(
            gpu.device(),
            wgpu::BufferUsages::INDIRECT | wgpu::BufferUsages::STORAGE,
        );
        let draw_cmd_bind_group = draw_cmd_buffer.create_storage_write_bind_group(&mut world);

        // let profiler = RefCell::new(GpuProfiler::new(wgpu_profiler::GpuProfilerSettings {
        //     enable_timer_queries: true,
        //     enable_debug_groups: true,
        //     max_num_pending_frames: 4,
        // })?);

        // let egui_renderer =
        //     egui_wgpu::Renderer::new(gpu.device(), ViewTarget::FORMAT, None, Self::SAMPLE_COUNT);
        // let egui_context = egui::Context::default();
        // {
        //     let mut arc_style = egui_context.style();
        //     let style = Arc::make_mut(&mut arc_style);
        //     style.visuals.window_shadow = egui::epaint::Shadow::NONE;
        //     egui_context.set_style(style.clone());
        // }
        // let viewport_id = egui_context.viewport_id();
        // let egui_state =
        //     egui_winit::State::new(egui_context.clone(), viewport_id, window, None, None);

        Ok(Self {
            // surface,
            surface_config,
            gbuffer,
            view_target,

            global_uniform,

            draw_cmd_buffer,
            draw_cmd_bind_group,

            // profiler,
            blitter: Blitter::new(&world),
            screenshot_ctx: ScreenshotCtx::new(&gpu, width, height),
            recorder: Recorder::new(),

            world,
            gpu,
            // egui_renderer,
            // egui_context,
            // egui_state,
        })
    }

    pub fn add_area_light(
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
            MeshPool::VERTICAL_PLANE_MESH,
            MaterialPool::LIGHT_MATERIAL,
        )]);
        Ok(())
    }

    pub fn setup_scene(&mut self, example: &mut impl Example) -> Result<()> {
        example.setup_scene(self)?;

        let mut encoder = self.device().create_command_encoder(&Default::default());
        self.draw_cmd_buffer.set_len(
            self.gpu.device(),
            &mut encoder,
            self.world.get_mut::<InstancePool>()?.count() as _,
        );

        self.draw_cmd_bind_group = self
            .draw_cmd_buffer
            .create_storage_write_bind_group(&mut self.world);

        let mut mesh_pool = self.get_mesh_pool_mut();
        mesh_pool.generate_tlas(&self.get_instance_pool().instances_data);

        mesh_pool.trace_bind_group = {
            let instance_pool = self.get_instance_pool();
            self.device().create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("Trace BG"),
                layout: &mesh_pool.trace_bind_group_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: mesh_pool.tlas_nodes.as_tight_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: instance_pool.instances.as_tight_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: mesh_pool.mesh_info.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: mesh_pool.bvh_nodes.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 4,
                        resource: mesh_pool.vertices.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 5,
                        resource: mesh_pool.indices.as_entire_binding(),
                    },
                ],
            })
        };

        Ok(())
    }

    pub fn render(
        &mut self,
        window: &Window,
        surface: &wgpu::Surface<'_>,
        app_state: &AppState,
        draw: impl FnOnce(RenderContext),
    ) -> Result<(), wgpu::SurfaceError> {
        // let mut profiler = self.profiler.borrow_mut();
        let target = surface.get_current_texture()?;
        let target_view = target.texture.create_view(&Default::default());

        let mut encoder = self
            .device()
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Main Render Encoder"),
            });

        // profiler.begin_scope("Main Render Scope ", &mut encoder, self.device());

        let render_context = RenderContext {
            window,
            app_state,
            encoder: ProfilerCommandEncoder {
                encoder: &mut encoder,
                device: self.gpu.device(),
                // profiler: &mut profiler,
            },
            view_target: &self.view_target,
            gbuffer: &self.gbuffer,
            world: &self.world,
            gpu: &self.gpu,
            width: self.surface_config.width,
            height: self.surface_config.height,
            draw_cmd_buffer: &self.draw_cmd_buffer,
            draw_cmd_bind_group: &self.draw_cmd_bind_group,
            // egui_context: &self.egui_context,
            // egui_renderer: &mut self.egui_renderer,
            // egui_state: &mut self.egui_state,
        };

        draw(render_context);

        self.blitter.blit_to_texture_with_binding(
            &mut encoder,
            self.world.device(),
            self.view_target.main_binding(),
            &target_view,
            self.surface_config.format,
        );

        // profiler.end_scope(&mut encoder);
        // profiler.resolve_queries(&mut encoder);

        self.gpu.queue().submit(Some(encoder.finish()));

        window.pre_present_notify();
        target.present();

        // profiler.end_frame().ok();

        if self.recorder.is_active() && self.recorder.ffmpeg_installed() {
            let tx = self.recorder.sender.clone();
            self.capture_frame(move |frame, _| {
                let _ = tx.send(RecordEvent::Record(frame));
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
        // self.surface
        //     .configure(self.gpu.device(), &self.surface_config);
        self.gbuffer.resize(&self.gpu, width, height);
        self.view_target = view_target::ViewTarget::new(&self.world, width, height);
        self.global_uniform.resolution = [width as f32, height as f32];

        self.screenshot_ctx.resize(&self.gpu, width, height);

        if self.recorder.is_active() {
            self.recorder.finish();
        }
    }

    pub fn update(
        &mut self,
        state: &mut AppState,
        actions: Vec<StateAction>,
        update: impl FnOnce(UpdateContext),
    ) -> Result<()> {
        // let mut profiler = self.profiler.borrow_mut();
        let mut encoder = self
            .device()
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Update Encoder"),
            });
        update(UpdateContext {
            app_state: state,
            encoder: ProfilerCommandEncoder {
                encoder: &mut encoder,
                device: self.device(),
                // profiler: &mut profiler,
            },
            world: &self.world,
            width: self.surface_config.width,
            height: self.surface_config.height,
        });
        self.gpu.queue().submit(Some(encoder.finish()));

        self.global_uniform.frame = state.frame_count as _;
        self.global_uniform.time = state.total_time as _;
        self.global_uniform.dt = state.dt as _;
        self.world
            .get_mut::<global_ubo::GlobalUniformBinding>()?
            .update(self.gpu.queue(), &self.global_uniform);

        let mut camera_uniform = self.world.unwrap_mut::<CameraUniform>();
        *camera_uniform = state.camera.get_uniform(Some(&camera_uniform));
        self.world
            .get_mut::<CameraUniformBinding>()?
            .update(self.gpu.queue(), &camera_uniform);

        // let timestamp_period = self.queue().get_timestamp_period();
        // if state.frame_count % 500 == 0 && std::env::var("GPU_PROFILING").is_ok() {
        //     let mut last_profile = vec![];
        //     while let Some(profiling_data) = profiler.process_finished_frame(timestamp_period) {
        //         last_profile = profiling_data;
        //     }
        //     scopes_to_console_recursive(&last_profile, 0);
        //     println!();
        // }

        for action in actions {
            match action {
                StateAction::StartRecording => {
                    self.recorder.start(self.screenshot_ctx.image_dimensions)
                }
                StateAction::FinishRecording => self.recorder.finish(),
                StateAction::Screenshot => {
                    let tx = self.recorder.sender.clone();
                    self.capture_frame(move |frame, dims| {
                        let _ = tx.send(RecordEvent::Screenshot((frame, dims)));
                    });
                }
            }
        }
        Ok(())
    }

    pub fn handle_events(&mut self, path: std::path::PathBuf) {
        self.get_pipeline_arena_mut().reload_pipelines(&path);
    }

    pub fn capture_frame(
        &self,
        callback: impl FnOnce(Arc<wgpu::Buffer>, ImageDimensions) + Send + 'static,
    ) {
        self.screenshot_ctx.capture_frame(
            &self.world,
            &self.blitter,
            self.view_target.main_binding(),
            callback,
        );
    }

    pub fn get_pipeline_arena(&self) -> Read<PipelineArena> {
        self.world.unwrap::<PipelineArena>()
    }

    pub fn get_pipeline_arena_mut(&self) -> Write<PipelineArena> {
        self.world.unwrap_mut::<PipelineArena>()
    }

    pub fn add_mesh(&mut self, mesh: MeshRef) -> MeshId {
        self.world.unwrap_mut::<MeshPool>().add(mesh)
    }

    pub fn get_material_pool(&self) -> Read<MaterialPool> {
        self.world.unwrap::<MaterialPool>()
    }

    pub fn get_material_pool_mut(&self) -> Write<MaterialPool> {
        self.world.unwrap_mut::<MaterialPool>()
    }

    pub fn get_texture_pool(&self) -> Read<TexturePool> {
        self.world.unwrap::<TexturePool>()
    }

    pub fn get_texture_pool_mut(&self) -> Write<TexturePool> {
        self.world.unwrap_mut::<TexturePool>()
    }

    pub fn get_mesh_pool(&self) -> Read<MeshPool> {
        self.world.unwrap::<MeshPool>()
    }

    pub fn get_mesh_pool_mut(&self) -> Write<MeshPool> {
        self.world.unwrap_mut::<MeshPool>()
    }

    pub fn get_instance_pool(&self) -> Read<InstancePool> {
        self.world.unwrap::<InstancePool>()
    }

    pub fn get_instance_pool_mut(&self) -> Write<InstancePool> {
        self.world.unwrap_mut::<InstancePool>()
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

pub struct UpdateContext<'a> {
    pub app_state: &'a mut AppState,
    pub encoder: ProfilerCommandEncoder<'a>,
    pub world: &'a World,
    pub width: u32,
    pub height: u32,
}

pub struct RenderContext<'a> {
    pub window: &'a Window,
    pub app_state: &'a AppState,
    pub encoder: ProfilerCommandEncoder<'a>,
    pub view_target: &'a ViewTarget,
    pub gbuffer: &'a GBuffer,
    pub world: &'a World,
    pub gpu: &'a Gpu,
    pub width: u32,
    pub height: u32,
    pub draw_cmd_buffer: &'a ResizableBuffer<DrawIndexedIndirect>,
    pub draw_cmd_bind_group: &'a wgpu::BindGroup,
    // egui_context: &'a egui::Context,
    // egui_renderer: &'a mut egui_wgpu::Renderer,
    // egui_state: &'a mut egui_winit::State,
}

impl<'a> RenderContext<'a> {
    // pub fn ui(&mut self, ui_builder: impl FnOnce(&egui::Context)) {
    //     let screen_descriptor = ScreenDescriptor {
    //         size_in_pixels: [self.width, self.height],
    //         pixels_per_point: self.egui_context.pixels_per_point(),
    //     };
    //
    //     let full_output = self
    //         .egui_context
    //         .run(self.egui_state.take_egui_input(self.window), |ctx| {
    //             ui_builder(ctx)
    //         });
    //
    //     let paint_jobs = self
    //         .egui_context
    //         .tessellate(full_output.shapes, self.egui_context.pixels_per_point());
    //     let textures_delta = full_output.textures_delta;
    //
    //     {
    //         for (texture_id, image_delta) in &textures_delta.set {
    //             self.egui_renderer.update_texture(
    //                 self.gpu.device(),
    //                 self.gpu.queue(),
    //                 *texture_id,
    //                 image_delta,
    //             );
    //         }
    //         for texture_id in &textures_delta.free {
    //             self.egui_renderer.free_texture(texture_id);
    //         }
    //         self.egui_renderer.update_buffers(
    //             self.gpu.device(),
    //             self.gpu.queue(),
    //             &mut self.encoder,
    //             &paint_jobs,
    //             &screen_descriptor,
    //         );
    //
    //         let mut render_pass = self.encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
    //             label: Some("UI Pass"),
    //             color_attachments: &[Some(wgpu::RenderPassColorAttachment {
    //                 view: self.view_target.main_view(),
    //                 resolve_target: None,
    //                 ops: wgpu::Operations {
    //                     load: wgpu::LoadOp::Load,
    //                     store: wgpu::StoreOp::Store,
    //                 },
    //             })],
    //             depth_stencil_attachment: None,
    //             timestamp_writes: None,
    //             occlusion_query_set: None,
    //         });
    //         self.egui_renderer
    //             .render(&mut render_pass, paint_jobs.as_slice(), &screen_descriptor);
    //     }
    // }
}

impl<'a> RenderContext<'a> {
    pub fn get_pipeline_arena(&self) -> Read<PipelineArena> {
        self.world.unwrap::<PipelineArena>()
    }
}

pub struct ProfilerCommandEncoder<'a> {
    encoder: &'a mut wgpu::CommandEncoder,

    _device: &'a wgpu::Device,
    // profiler: &'a mut GpuProfiler,
}

impl<'a> ProfilerCommandEncoder<'a> {
    pub fn profile_start(&mut self, label: &str) {
        #[cfg(debug_assertions)]
        self.encoder.push_debug_group(label);
        // self.profiler.begin_scope(label, self.encoder, self.device);
    }

    pub fn profile_end(&mut self) {
        // self.profiler.end_scope(self.encoder);
        // #[cfg(debug_assertions)]
        self.encoder.pop_debug_group();
    }

    pub fn begin_compute_pass(
        &mut self,
        desc: &wgpu::ComputePassDescriptor,
        // ) -> wgpu_profiler::scope::OwningScope<wgpu::ComputePass> {
    ) -> ComputePass {
        // wgpu_profiler::scope::OwningScope::start(
        //     desc.label.unwrap_or("Compute Pass"),
        //     // self.profiler,
        //     self.encoder.begin_compute_pass(desc),
        //     self.device,
        // )
        self.encoder.begin_compute_pass(desc)
    }

    pub fn begin_render_pass<'pass>(
        &'pass mut self,
        desc: &wgpu::RenderPassDescriptor<'pass, '_>,
        // ) -> wgpu_profiler::scope::OwningScope<wgpu::RenderPass<'pass>> {
    ) -> RenderPass<'pass> {
        // wgpu_profiler::scope::OwningScope::start(
        //     desc.label.unwrap_or("Render Pass"),
        //     // self.profiler,
        //     self.encoder.begin_render_pass(desc),
        //     self.device,
        // )

        self.encoder.begin_render_pass(desc)
    }
}

impl<'a> std::ops::Deref for ProfilerCommandEncoder<'a> {
    type Target = wgpu::CommandEncoder;

    fn deref(&self) -> &Self::Target {
        self.encoder
    }
}
impl<'a> std::ops::DerefMut for ProfilerCommandEncoder<'a> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.encoder
    }
}

// pub fn scopes_to_console_recursive(results: &[GpuTimerScopeResult], indentation: usize) {
//     for scope in results {
//         if indentation > 0 {
//             print!("{:<width$}", "|", width = 4 * indentation);
//         }
//         let time = Duration::from_micros(((scope.time.end - scope.time.start) * 1e6) as u64);
//         println!("{time:?} - {}", scope.label);
//         if !scope.nested_scopes.is_empty() {
//             scopes_to_console_recursive(&scope.nested_scopes, indentation + 1);
//         }
//     }
// }

fn preferred_framebuffer_format(formats: &[wgpu::TextureFormat]) -> wgpu::TextureFormat {
    for &format in formats {
        if matches!(
            format,
            wgpu::TextureFormat::Rgba8Unorm | wgpu::TextureFormat::Bgra8Unorm
        ) {
            return format;
        }
    }
    formats[0]
}
