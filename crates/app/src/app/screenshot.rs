use std::sync::Arc;

use wgpu::MapMode;

use crate::Gpu;

use components::{world::World, Blitter, ImageDimensions};

pub struct ScreenshotCtx {
    pub image_dimensions: ImageDimensions,
    texture: wgpu::Texture,
}

impl ScreenshotCtx {
    pub fn new(gpu: &Gpu, width: u32, height: u32) -> Self {
        let image_dimensions =
            ImageDimensions::new(width, height, wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);

        let texture = gpu.device().create_texture(&wgpu::TextureDescriptor {
            label: Some("Screen Copy Texture"),
            size: image_dimensions.into(),
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            mip_level_count: 1,
            sample_count: 1,
            view_formats: &[],
        });

        Self {
            image_dimensions,
            texture,
        }
    }

    pub fn resize(&mut self, gpu: &Gpu, width: u32, height: u32) {
        let new_dims = ImageDimensions::new(width, height, wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);

        self.texture = gpu.device().create_texture(&wgpu::TextureDescriptor {
            label: Some("Screen Copy Texture"),
            size: wgpu::Extent3d {
                width: new_dims.width,
                height: new_dims.height,
                depth_or_array_layers: 1,
            },
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            mip_level_count: 1,
            sample_count: 1,
            view_formats: &[],
        });
        self.image_dimensions = new_dims;
    }

    pub fn capture_frame(
        &self,
        world: &World,
        blitter: &Blitter,
        src_texture: &wgpu::BindGroup,
        callback: impl FnOnce(Arc<wgpu::Buffer>, ImageDimensions) + Send + 'static,
    ) {
        let dims = self.image_dimensions;

        let download = Arc::new(world.device().create_buffer(&wgpu::BufferDescriptor {
            size: dims.linear_size(),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
            label: Some("Download Buffer"),
        }));

        let view = self.texture.create_view(&Default::default());
        let mut encoder = world
            .device()
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Screenshot"),
            });
        blitter.blit_to_texture_with_binding(
            &mut encoder,
            world.device(),
            src_texture,
            &view,
            self.texture.format(),
        );

        encoder.copy_texture_to_buffer(
            self.texture.as_image_copy(),
            wgpu::ImageCopyBuffer {
                buffer: &download,
                layout: wgpu::ImageDataLayout {
                    offset: 0,
                    bytes_per_row: Some(dims.padded_bytes_per_row),
                    rows_per_image: None,
                },
            },
            self.texture.size(),
        );

        world.queue().submit(Some(encoder.finish()));

        let buff = download.clone();
        let image_slice = download.slice(0..dims.linear_size());
        image_slice.map_async(MapMode::Read, move |res| {
            if let Err(err) = res {
                log::error!("Oh no, failed to map buffer: {err}");
                return;
            }

            callback(buff, dims);
        });
    }
}
