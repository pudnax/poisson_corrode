use crate::Gpu;

use components::bind_group_layout::{self, WrappedBindGroupLayout};

pub struct GBuffer {
    pub normal_uv: wgpu::TextureView,
    pub material: wgpu::TextureView,
    pub depth: wgpu::TextureView,

    pub bind_group: wgpu::BindGroup,
    pub bind_group_layout: bind_group_layout::BindGroupLayout,
}

impl GBuffer {
    pub const NORMAL_UV_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rg32Uint;
    pub const MATERIAL_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::R8Uint;
    pub const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth24Plus;
    pub const fn color_target_state() -> &'static [Option<wgpu::ColorTargetState>] {
        &[
            Some(wgpu::ColorTargetState {
                format: Self::NORMAL_UV_FORMAT,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            }),
            Some(wgpu::ColorTargetState {
                format: Self::MATERIAL_FORMAT,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            }),
        ]
    }

    pub fn color_target_attachment(&self) -> [Option<wgpu::RenderPassColorAttachment>; 2] {
        [&self.normal_uv, &self.material].map(|view| {
            Some(wgpu::RenderPassColorAttachment {
                view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })
        })
    }

    const LAYOUT_DESC: wgpu::BindGroupLayoutDescriptor<'static> = wgpu::BindGroupLayoutDescriptor {
        label: Some("GBuffer Bind Group Layout"),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT.union(wgpu::ShaderStages::COMPUTE),
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Uint,
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT.union(wgpu::ShaderStages::COMPUTE),
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Uint,
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 2,
                visibility: wgpu::ShaderStages::FRAGMENT.union(wgpu::ShaderStages::COMPUTE),
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Depth,
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 3,
                visibility: wgpu::ShaderStages::FRAGMENT.union(wgpu::ShaderStages::COMPUTE),
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
        ],
    };

    pub fn new(gpu: &Gpu, width: u32, height: u32) -> Self {
        let size = wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        };
        let mut desc = wgpu::TextureDescriptor {
            label: Some("GBuffer: normal and uv"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: Self::NORMAL_UV_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        };
        let normal_uv = create_view(gpu, &desc);

        desc.label = Some("GBuffer: material");
        desc.format = Self::MATERIAL_FORMAT;
        let material = create_view(gpu, &desc);

        desc.label = Some("GBuffer: depth");
        desc.format = Self::DEPTH_FORMAT;
        let depth_tex = gpu.device().create_texture(&desc);
        let depth = depth_tex.create_view(&Default::default());

        let bind_group_layout = gpu
            .device()
            .create_bind_group_layout_wrap(&Self::LAYOUT_DESC);

        let sampler = gpu
            .device()
            .create_sampler(&crate::app::DEFAULT_SAMPLER_DESC);

        let bind_group = gpu.device().create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("GBuffer: bind group"),
            layout: &bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&normal_uv),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&material),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&depth_tex.create_view(
                        &wgpu::TextureViewDescriptor {
                            aspect: wgpu::TextureAspect::DepthOnly,
                            ..Default::default()
                        },
                    )),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        });

        Self {
            normal_uv,
            material,
            depth,

            bind_group_layout,
            bind_group,
        }
    }

    pub fn resize(&mut self, gpu: &Gpu, width: u32, height: u32) {
        let mut other = Self::new(gpu, width, height);
        std::mem::swap(self, &mut other);
    }
}

fn create_view(gpu: &Gpu, desc: &wgpu::TextureDescriptor) -> wgpu::TextureView {
    gpu.device()
        .create_texture(desc)
        .create_view(&Default::default())
}
