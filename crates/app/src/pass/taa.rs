use std::{
    path::Path,
    sync::atomic::{AtomicU8, Ordering},
};

use crate::{
    pipeline::{ComputeHandle, ComputePipelineDescriptor, PipelineArena},
    CameraUniformBinding, GBuffer, ProfilerCommandEncoder, ViewTarget, DEFAULT_SAMPLER_DESC,
};
use eyre::Result;
use components::{
    bind_group_layout::{BindGroupLayout, SingleTextureBindGroupLayout, WrappedBindGroupLayout},
    world::World,
};
use glam::{vec2, Vec2};
use rand::{rngs::SmallRng, seq::SliceRandom, SeedableRng};
use wgpu::util::align_to;

use super::Pass;

struct CombinedTexture {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    sample_bind_group: wgpu::BindGroup,
    storage_bind_group: wgpu::BindGroup,
}

#[inline]
fn radical_inverse(mut n: u32, base: u32) -> f32 {
    let mut val = 0.0f32;
    let inv_base = 1.0f32 / base as f32;
    let mut inv_bi = inv_base;

    while n > 0 {
        let d_i = n % base;
        val += d_i as f32 * inv_bi;
        n = (n as f32 * inv_base) as u32;
        inv_bi *= inv_base;
    }

    val
}

impl CombinedTexture {
    pub fn new(
        device: &wgpu::Device,
        width: u32,
        height: u32,
        format: wgpu::TextureFormat,
        read_bgl: &wgpu::BindGroupLayout,
        write_bgl: &wgpu::BindGroupLayout,
        label: Option<&str>,
    ) -> Self {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label,
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::STORAGE_BINDING
                | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&Default::default());
        let sample_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Read Texture BG"),
            layout: read_bgl,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&view),
            }],
        });
        let storage_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Write Texture BG"),
            layout: write_bgl,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&view),
            }],
        });

        Self {
            texture,
            view,
            sample_bind_group,
            storage_bind_group,
        }
    }
}

pub struct Taa {
    read_texture_layout: BindGroupLayout,
    write_texture_layout: BindGroupLayout,

    active_texture: AtomicU8,
    history: [CombinedTexture; 2],
    motion_texture: CombinedTexture,

    reprojection_pipeline: ComputeHandle,
    taa_pipeline: ComputeHandle,
    sampler: wgpu::BindGroup,

    jitter_samples: Vec<Vec2>,
}

impl Taa {
    pub fn new(world: &World, gbuffer: &GBuffer, width: u32, height: u32) -> Result<Self> {
        let device = world.gpu.device();
        let mut pipeline_arena = world.get_mut::<PipelineArena>()?;
        let camera_binding = world.get::<CameraUniformBinding>()?;
        let sampler_layout =
            device.create_bind_group_layout_wrap(&wgpu::BindGroupLayoutDescriptor {
                label: Some("Sampler BGL"),
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                }],
            });
        let input_texture_layout = world.get::<SingleTextureBindGroupLayout>()?;

        let read_texture_layout =
            device.create_bind_group_layout_wrap(&wgpu::BindGroupLayoutDescriptor {
                label: Some("History Texture BGL"),
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                }],
            });
        let write_texture_layout =
            device.create_bind_group_layout_wrap(&wgpu::BindGroupLayoutDescriptor {
                label: Some("History Texture BGL"),
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::StorageTexture {
                        access: wgpu::StorageTextureAccess::WriteOnly,
                        format: wgpu::TextureFormat::Rgba16Float,
                        view_dimension: wgpu::TextureViewDimension::D2,
                    },
                    count: None,
                }],
            });

        let history_textures = std::array::from_fn(|i| {
            CombinedTexture::new(
                device,
                width,
                height,
                wgpu::TextureFormat::Rgba16Float,
                &read_texture_layout,
                &write_texture_layout,
                Some(&format!("History Texture {i}")),
            )
        });

        let motion_texture = CombinedTexture::new(
            device,
            width,
            height,
            wgpu::TextureFormat::Rgba16Float,
            &read_texture_layout,
            &write_texture_layout,
            Some("Motion Texture"),
        );

        let pipeline_desc = ComputePipelineDescriptor {
            label: Some("Reprojection Pipeline".into()),
            layout: vec![
                camera_binding.bind_group_layout.clone(),
                gbuffer.bind_group_layout.clone(),
                write_texture_layout.clone(),
            ],
            ..Default::default()
        };
        let shader_path = Path::new("shaders").join("reproject.wgsl");
        let reprojection_pipeline =
            pipeline_arena.process_compute_pipeline_from_path(shader_path, pipeline_desc)?;

        let pipeline_desc = ComputePipelineDescriptor {
            label: Some("Taa Pipeline".into()),
            layout: vec![
                sampler_layout.clone(),
                // Input Texture
                input_texture_layout.layout.clone(),
                // History Texture
                read_texture_layout.clone(),
                // Motion Texture
                read_texture_layout.clone(),
                // Output Texture
                write_texture_layout.clone(),
            ],
            ..Default::default()
        };
        let shader_path = Path::new("shaders").join("taa.wgsl");
        let taa_pipeline =
            pipeline_arena.process_compute_pipeline_from_path(shader_path, pipeline_desc)?;

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("Taa Sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..DEFAULT_SAMPLER_DESC
        });
        let sampler = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Sampler BG"),
            layout: &sampler_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::Sampler(&sampler),
            }],
        });

        let n = 16;
        let jitter_samples = (0..n)
            .map(|i| {
                Vec2::new(
                    radical_inverse(i % n + 1, 2) * 2. - 1.,
                    radical_inverse(i % n + 1, 3) * 2. - 1.,
                )
            })
            .collect();

        Ok(Self {
            read_texture_layout,
            write_texture_layout,

            active_texture: AtomicU8::new(0),
            history: history_textures,
            motion_texture,

            reprojection_pipeline,
            taa_pipeline,
            sampler,

            jitter_samples,
        })
    }

    pub fn resize(&mut self, device: &wgpu::Device, width: u32, height: u32) {
        self.history = std::array::from_fn(|i| {
            CombinedTexture::new(
                device,
                width,
                height,
                wgpu::TextureFormat::Rgba16Float,
                &self.read_texture_layout,
                &self.write_texture_layout,
                Some(&format!("History Texture {i}")),
            )
        });

        self.motion_texture = CombinedTexture::new(
            device,
            width,
            height,
            wgpu::TextureFormat::Rgba16Float,
            &self.read_texture_layout,
            &self.write_texture_layout,
            Some("Motion Texture"),
        );
    }

    pub fn output_texture(&self) -> &wgpu::TextureView {
        &self.history[self.active_texture.load(Ordering::Relaxed) as usize].view
    }

    pub fn get_jitter(&mut self, frame_idx: u32, width: u32, height: u32) -> Vec2 {
        if 0 == frame_idx % self.jitter_samples.len() as u32 && frame_idx > 0 {
            let mut rng = SmallRng::seed_from_u64(frame_idx as u64);

            let prev_sample = self.jitter_samples.last().copied();
            loop {
                self.jitter_samples.shuffle(&mut rng);
                if self.jitter_samples.first().copied() != prev_sample {
                    break;
                }
            }
        }

        self.jitter_samples[frame_idx as usize % self.jitter_samples.len()]
            / vec2(width as f32, height as f32)
    }
}

pub struct TaaResource<'a> {
    pub view_target: &'a ViewTarget,
    pub gbuffer: &'a GBuffer,
    pub width_height: (u32, u32),
}

impl Pass for Taa {
    type Resources<'a> = TaaResource<'a>;

    fn record(
        &self,
        world: &World,
        encoder: &mut ProfilerCommandEncoder,
        resource: Self::Resources<'_>,
    ) {
        let input_history = self.active_texture.fetch_xor(1, Ordering::Relaxed) as usize;
        let output_history = input_history ^ 1;

        let camera = world.unwrap::<CameraUniformBinding>();
        let arena = world.unwrap::<PipelineArena>();

        let (width, height) = resource.width_height;
        let x = align_to(width, 8) / 8;
        let y = align_to(height, 8) / 8;

        let mut cpass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("Reprojection Pass"),
            timestamp_writes: None,
        });

        cpass.set_pipeline(arena.get_pipeline(self.reprojection_pipeline));
        cpass.set_bind_group(0, &camera.binding, &[]);
        cpass.set_bind_group(1, &resource.gbuffer.bind_group, &[]);
        cpass.set_bind_group(2, &self.motion_texture.storage_bind_group, &[]);
        cpass.dispatch_workgroups(x, y, 1);
        drop(cpass);

        let mut cpass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("Taa Pass"),
            timestamp_writes: None,
        });

        cpass.set_pipeline(arena.get_pipeline(self.taa_pipeline));
        cpass.set_bind_group(0, &self.sampler, &[]);
        cpass.set_bind_group(1, resource.view_target.main_binding(), &[]);
        cpass.set_bind_group(2, &self.history[input_history].sample_bind_group, &[]);
        cpass.set_bind_group(3, &self.motion_texture.sample_bind_group, &[]);
        cpass.set_bind_group(4, &self.history[output_history].storage_bind_group, &[]);
        cpass.dispatch_workgroups(x, y, 1);
        drop(cpass);

        encoder.copy_texture_to_texture(
            self.history[output_history].texture.as_image_copy(),
            resource.view_target.main_texture().as_image_copy(),
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
    }
}
