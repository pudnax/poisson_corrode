use std::sync::Arc;

use wgpu::util::DeviceExt;

use components::{
    bind_group_layout::{self, WrappedBindGroupLayout},
    create_solid_color_texture, Gpu,
};

pub const WHITE_TEXTURE: TextureId = TextureId(0);
pub const BLACK_TEXTURE: TextureId = TextureId(1);
pub const LTC1_TEXTURE: TextureId = TextureId(2);
pub const LTC2_TEXTURE: TextureId = TextureId(3);

mod ltc {
    include!("ltc_matrix.raw");
}

#[repr(C)]
#[derive(Debug, Copy, Default, Clone, bytemuck::Pod, bytemuck::Zeroable)]
pub struct TextureId(u32);

impl TextureId {
    pub fn id(&self) -> u32 {
        self.0
    }
}

pub struct TexturePool {
    pub views: Vec<wgpu::TextureView>,

    sampler: wgpu::Sampler,
    ltc_sampler: wgpu::Sampler,
    pub bind_group_layout: bind_group_layout::BindGroupLayout,
    pub bind_group: wgpu::BindGroup,

    gpu: Arc<Gpu>,
}

const MAX_TEXTURES: u32 = 1 << 10;

impl TexturePool {
    pub fn new(gpu: Arc<Gpu>) -> Self {
        let views = default_textures(&gpu);

        let bind_group_layout =
            gpu.device()
                .create_bind_group_layout_wrap(&wgpu::BindGroupLayoutDescriptor {
                    label: Some("TexturePool: bind group layout"),
                    entries: &[
                        wgpu::BindGroupLayoutEntry {
                            binding: 0,
                            visibility: wgpu::ShaderStages::VERTEX_FRAGMENT
                                | wgpu::ShaderStages::COMPUTE,
                            ty: wgpu::BindingType::Texture {
                                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                                view_dimension: wgpu::TextureViewDimension::D2,
                                multisampled: false,
                            },
                            count: core::num::NonZeroU32::new(MAX_TEXTURES),
                        },
                        wgpu::BindGroupLayoutEntry {
                            binding: 1,
                            visibility: wgpu::ShaderStages::VERTEX_FRAGMENT
                                | wgpu::ShaderStages::COMPUTE,
                            ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                            count: None,
                        },
                        wgpu::BindGroupLayoutEntry {
                            binding: 2,
                            visibility: wgpu::ShaderStages::VERTEX_FRAGMENT
                                | wgpu::ShaderStages::COMPUTE,
                            ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                            count: None,
                        },
                    ],
                });
        let sampler = gpu.device().create_sampler(&wgpu::SamplerDescriptor {
            label: Some("Gltf Default Sampler"),
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
        let ltc_sampler = gpu.device().create_sampler(&wgpu::SamplerDescriptor {
            label: Some("Ltc Sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Nearest,
            mipmap_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });

        let bind_group =
            Self::create_bind_group(&gpu, &bind_group_layout, &views, &sampler, &ltc_sampler);

        Self {
            views,

            sampler,
            ltc_sampler,
            bind_group_layout,
            bind_group,
            gpu,
        }
    }

    pub fn add(&mut self, view: wgpu::TextureView) -> TextureId {
        self.views.push(view);

        TextureId(self.views.len() as u32 - 1)
    }

    fn create_bind_group(
        gpu: &Gpu,
        bind_group_layout: &wgpu::BindGroupLayout,
        views: &[wgpu::TextureView],
        sampler: &wgpu::Sampler,
        ltc_sampler: &wgpu::Sampler,
    ) -> wgpu::BindGroup {
        let views: Vec<_> = views.iter().collect();

        gpu.device().create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("TexturePool: bind group"),
            layout: bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureViewArray(&views),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(ltc_sampler),
                },
            ],
        })
    }

    pub fn update_bind_group(&mut self) {
        self.bind_group = Self::create_bind_group(
            &self.gpu,
            &self.bind_group_layout,
            &self.views,
            &self.sampler,
            &self.ltc_sampler,
        )
    }
}

fn default_textures(gpu: &Gpu) -> Vec<wgpu::TextureView> {
    let white = create_solid_color_texture(gpu.device(), gpu.queue(), glam::Vec3::splat(1.))
        .create_view(&Default::default());
    let black = create_solid_color_texture(gpu.device(), gpu.queue(), glam::Vec3::splat(0.))
        .create_view(&Default::default());

    let mut ltc_desc = wgpu::TextureDescriptor {
        label: Some("LTC 1"),
        size: wgpu::Extent3d {
            width: 64,
            height: 64,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba32Float,
        usage: wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    };
    let ltc1 = gpu
        .device()
        .create_texture_with_data(
            gpu.queue(),
            &ltc_desc,
            wgpu::util::TextureDataOrder::LayerMajor,
            bytemuck::cast_slice(ltc::LTC1),
        )
        .create_view(&Default::default());
    ltc_desc.label = Some("LTC 2");
    let ltc2 = gpu
        .device()
        .create_texture_with_data(
            gpu.queue(),
            &ltc_desc,
            wgpu::util::TextureDataOrder::LayerMajor,
            bytemuck::cast_slice(ltc::LTC2),
        )
        .create_view(&Default::default());

    vec![white, black, ltc1, ltc2]
}
