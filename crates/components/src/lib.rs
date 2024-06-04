#![allow(clippy::new_without_default)]

use std::{
    io,
    iter::{self, Repeat},
    num::NonZeroU64,
    ops::Range,
    path::Path,
};

pub mod bind_group_layout;
mod blitter;
mod buffer;
mod camera;
mod fps_counter;
mod import_resolver;
mod input;
mod recorder;
pub mod shared;
mod watcher;
pub mod world;

pub use shared::*;

pub use bind_group_layout::{BindGroupLayout, WrappedBindGroupLayout};
pub use blitter::Blitter;
pub use buffer::{ResizableBuffer, ResizableBufferExt};
pub use camera::{Camera, CameraUniform, CameraUniformBinding};
pub use fps_counter::FpsCounter;
pub use import_resolver::{ImportResolver, ResolvedFile};
pub use input::{Input, KeyMap, KeyboardMap, KeyboardState};
pub use recorder::{RecordEvent, Recorder};
pub use watcher::Watcher;
pub use world::World;

use either::Either;
use glam::Vec3;
use wgpu::util::{align_to, DeviceExt};

pub const SCREENSHOTS_FOLDER: &str = "screenshots";
pub const VIDEO_FOLDER: &str = "recordings";

#[derive(Debug)]
pub struct Gpu {
    adapter: wgpu::Adapter,
    device: wgpu::Device,
    queue: wgpu::Queue,
}

impl Gpu {
    pub fn new(adapter: wgpu::Adapter, device: wgpu::Device, queue: wgpu::Queue) -> Self {
        Self {
            adapter,
            device,
            queue,
        }
    }

    pub fn device(&self) -> &wgpu::Device {
        &self.device
    }

    pub fn queue(&self) -> &wgpu::Queue {
        &self.queue
    }

    pub fn adapter(&self) -> &wgpu::Adapter {
        &self.adapter
    }
}

pub trait NonZeroSized: Sized {
    const NSIZE: NonZeroU64 = { unsafe { NonZeroU64::new_unchecked(Self::SIZE as _) } };
    const SIZE: usize = {
        if std::mem::size_of::<Self>() == 0 {
            panic!("type is zero-sized");
        }
        std::mem::size_of::<Self>()
    };
}
impl<T> NonZeroSized for T where T: Sized {}

pub trait LerpExt {
    fn lerp(self, rhs: Self, t: Self) -> Self;
}

impl LerpExt for f32 {
    fn lerp(self, rhs: Self, t: Self) -> Self {
        self * (1. - t) + rhs * t
    }
}

impl LerpExt for f64 {
    fn lerp(self, rhs: Self, t: Self) -> Self {
        self * (1. - t) + rhs * t
    }
}

#[repr(C)]
#[derive(bytemuck::Pod, bytemuck::Zeroable, Clone, Copy, Debug)]
pub struct DrawIndexedIndirect {
    pub vertex_count: u32,
    pub instance_count: u32,
    pub base_index: u32,
    pub vertex_offset: i32,
    pub base_instance: u32,
}

pub trait Lerp: Sized {
    fn lerp(self, range: Range<Self>) -> Self;
}

impl Lerp for f32 {
    fn lerp(self, Range { start: a, end: b }: Range<Self>) -> Self {
        a * (1. - self) + b * self
    }
}

impl Lerp for f64 {
    fn lerp(self, Range { start: a, end: b }: Range<Self>) -> Self {
        a * (1. - self) + b * self
    }
}

pub trait UnwrapRepeat<T: Default + Clone, I>
where
    I: Iterator<Item = T>,
{
    fn unwrap_repeat(self) -> Either<I, Repeat<T>>;
}

impl<T: Default + Clone, I> UnwrapRepeat<T, I> for Option<I>
where
    I: Iterator<Item = T>,
{
    fn unwrap_repeat(self) -> Either<I, Repeat<T>> {
        match self {
            Some(iter) => Either::Left(iter),
            None => Either::Right(iter::repeat(T::default())),
        }
    }
}

/// Creates WGPU texture with color in range [0., 1.]
pub fn create_solid_color_texture(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    color: Vec3,
) -> wgpu::Texture {
    let color = color.extend(1.);
    let color = color.to_array().map(|val| (255. * val.clamp(0., 1.)) as u8);

    device.create_texture_with_data(
        queue,
        &wgpu::TextureDescriptor {
            label: Some(&format!("Solid Texture {:?}", color)),
            size: wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING,
            mip_level_count: 1,
            sample_count: 1,
            view_formats: &[],
        },
        wgpu::util::TextureDataOrder::LayerMajor,
        bytemuck::bytes_of(&color),
    )
}

#[derive(Debug, Clone, Copy)]
pub struct ImageDimensions {
    pub width: u32,
    pub height: u32,
    pub unpadded_bytes_per_row: u32,
    pub padded_bytes_per_row: u32,
}

impl ImageDimensions {
    pub fn new(width: u32, height: u32, align: u32) -> Self {
        let width = align_to(width, 2);
        let height = align_to(height, 2);
        let bytes_per_pixel = std::mem::size_of::<[u8; 4]>() as u32;
        let unpadded_bytes_per_row = width * bytes_per_pixel;
        let row_padding = (align - unpadded_bytes_per_row % align) % align;
        let padded_bytes_per_row = unpadded_bytes_per_row + row_padding;
        Self {
            width,
            height,
            unpadded_bytes_per_row,
            padded_bytes_per_row,
        }
    }

    pub fn linear_size(&self) -> u64 {
        self.padded_bytes_per_row as u64 * self.height as u64
    }
}

impl From<ImageDimensions> for wgpu::Extent3d {
    fn from(value: ImageDimensions) -> Self {
        wgpu::Extent3d {
            width: value.width,
            height: value.height,
            depth_or_array_layers: 1,
        }
    }
}

pub fn create_folder(name: impl AsRef<Path>) -> io::Result<()> {
    match std::fs::create_dir(name) {
        Ok(_) => {}
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
        Err(e) => return Err(e),
    }

    Ok(())
}

pub trait FormatConversions {
    fn swap_srgb_suffix(self) -> wgpu::TextureFormat;
}

impl FormatConversions for wgpu::TextureFormat {
    fn swap_srgb_suffix(self) -> wgpu::TextureFormat {
        use wgpu::TextureFormat::*;
        match self {
            Rgba8UnormSrgb => Rgba8Unorm,
            Bgra8UnormSrgb => Bgra8Unorm,
            Bc1RgbaUnormSrgb => Bc1RgbaUnorm,
            Bc2RgbaUnormSrgb => Bc2RgbaUnorm,
            Bc3RgbaUnormSrgb => Bc3RgbaUnorm,
            Bc7RgbaUnormSrgb => Bc7RgbaUnorm,
            Etc2Rgb8UnormSrgb => Etc2Rgb8Unorm,
            Etc2Rgb8A1UnormSrgb => Etc2Rgb8A1Unorm,
            Etc2Rgba8UnormSrgb => Etc2Rgba8Unorm,
            Astc {
                block,
                channel: wgpu::AstcChannel::UnormSrgb,
            } => Astc {
                block,
                channel: wgpu::AstcChannel::Unorm,
            },

            Rgba8Unorm => Rgba8UnormSrgb,
            Bgra8Unorm => Bgra8UnormSrgb,
            Bc1RgbaUnorm => Bc1RgbaUnormSrgb,
            Bc2RgbaUnorm => Bc2RgbaUnormSrgb,
            Bc3RgbaUnorm => Bc3RgbaUnormSrgb,
            Bc7RgbaUnorm => Bc7RgbaUnormSrgb,
            Etc2Rgb8Unorm => Etc2Rgb8UnormSrgb,
            Etc2Rgb8A1Unorm => Etc2Rgb8A1UnormSrgb,
            Etc2Rgba8Unorm => Etc2Rgba8UnormSrgb,
            Astc {
                block,
                channel: wgpu::AstcChannel::Unorm,
            } => Astc {
                block,
                channel: wgpu::AstcChannel::UnormSrgb,
            },
            _ => self,
        }
    }
}
