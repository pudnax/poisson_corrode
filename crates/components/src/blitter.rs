use std::cell::RefCell;

use ahash::AHashMap;
use wgpu::PipelineCompilationOptions;

use crate::world::World;

use super::bind_group_layout::SingleTextureBindGroupLayout;

pub struct Blitter {
    pipelines: RefCell<AHashMap<(wgpu::TextureFormat, bool), wgpu::RenderPipeline>>,
    shader: wgpu::ShaderModule,
    pipeline_layout: wgpu::PipelineLayout,
    sampler: wgpu::BindGroup,
}

impl Blitter {
    pub fn new(world: &World) -> Self {
        let device = world.device();
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Blit Shader"),
            source: wgpu::ShaderSource::Wgsl(std::borrow::Cow::Borrowed(include_str!("blit.wgsl"))),
        });
        let texture_bind_group_layout = world.unwrap::<SingleTextureBindGroupLayout>();

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("Blit Sampler"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            address_mode_w: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Linear,
            lod_min_clamp: 0.0,
            lod_max_clamp: f32::MAX,
            compare: None,
            anisotropy_clamp: 1,
            border_color: None,
        });
        let sampler_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("Blit Sampler Group Layout"),
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                }],
            });
        let sampler = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Blit Sampler Bind Group"),
            layout: &sampler_bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::Sampler(&sampler),
            }],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Blit Pipeline Layout"),
            bind_group_layouts: &[&texture_bind_group_layout, &sampler_bind_group_layout],
            push_constant_ranges: &[],
        });

        Self {
            pipelines: RefCell::new(AHashMap::new()),
            shader,
            pipeline_layout,
            sampler,
        }
    }

    pub fn blit_to_texture_with_binding(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        device: &wgpu::Device,
        src_texture: &wgpu::BindGroup,
        dst_texture: &wgpu::TextureView,
        dst_format: wgpu::TextureFormat,
    ) {
        let mut pipelines = self.pipelines.borrow_mut();
        let pipeline = pipelines.entry((dst_format, false)).or_insert_with(|| {
            Self::create_pipeline(
                device,
                &self.shader,
                dst_format,
                &self.pipeline_layout,
                false,
            )
        });

        let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Blit Pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: dst_texture,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::WHITE),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });

        render_pass.set_pipeline(pipeline);
        render_pass.set_bind_group(0, src_texture, &[]);
        render_pass.set_bind_group(1, &self.sampler, &[]);
        render_pass.draw(0..3, 0..1);
    }

    pub fn blit_to_texture(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        world: &World,
        src_texture: &wgpu::TextureView,
        dst_texture: &wgpu::TextureView,
        dst_format: wgpu::TextureFormat,
    ) {
        let texture_bind_group_layout = world.unwrap::<SingleTextureBindGroupLayout>();
        let texture_bind_group = world
            .device()
            .create_bind_group(&wgpu::BindGroupDescriptor {
                label: None,
                layout: &texture_bind_group_layout,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(src_texture),
                }],
            });

        self.blit_to_texture_with_binding(
            encoder,
            world.device(),
            &texture_bind_group,
            dst_texture,
            dst_format,
        )
    }

    pub fn generate_mipmaps(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        world: &World,
        texture: &wgpu::Texture,
    ) {
        let mip_count = texture.mip_level_count();

        let views: Vec<_> = (0..mip_count)
            .map(|base_mip_level| {
                texture.create_view(&wgpu::TextureViewDescriptor {
                    base_mip_level,
                    mip_level_count: Some(1),
                    ..Default::default()
                })
            })
            .collect();

        for (src_view, dst_view) in views.iter().zip(views.iter().skip(1)) {
            let texture_bind_group_layout = world.unwrap::<SingleTextureBindGroupLayout>();
            let texture_bind_group = world
                .device()
                .create_bind_group(&wgpu::BindGroupDescriptor {
                    label: None,
                    layout: &texture_bind_group_layout,
                    entries: &[wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(src_view),
                    }],
                });
            let mut pipelines = self.pipelines.borrow_mut();
            let pipeline = pipelines
                .entry((texture.format(), true))
                .or_insert_with(|| {
                    Self::create_pipeline(
                        world.device(),
                        &self.shader,
                        texture.format(),
                        &self.pipeline_layout,
                        true,
                    )
                });

            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Blit Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: dst_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::WHITE),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });

            render_pass.set_pipeline(pipeline);
            render_pass.set_bind_group(0, &texture_bind_group, &[]);
            render_pass.set_bind_group(1, &self.sampler, &[]);
            render_pass.draw(0..3, 0..1);
        }
    }

    fn create_pipeline(
        device: &wgpu::Device,
        shader: &wgpu::ShaderModule,
        format: wgpu::TextureFormat,
        layout: &wgpu::PipelineLayout,
        mipmap: bool,
    ) -> wgpu::RenderPipeline {
        let entry_point = if mipmap {
            "fs_main"
        } else {
            match format {
                wgpu::TextureFormat::Rgba8UnormSrgb => "fs_main",
                wgpu::TextureFormat::Bgra8UnormSrgb => "fs_main",
                _ => "fs_main_srgb",
            }
        };
        device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Blit Pipeline"),
            layout: Some(layout),
            vertex: wgpu::VertexState {
                module: shader,
                entry_point: "vs_main",
                buffers: &[],
                compilation_options: PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: shader,
                entry_point,
                targets: &[Some(format.into())],
                compilation_options: PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
        })
    }
}
