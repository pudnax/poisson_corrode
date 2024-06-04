use std::mem::size_of;

use eyre::{eyre, ContextCompat, Result};
use gltf::{
    accessor::{DataType, Dimensions},
    image::Format,
    texture::{MagFilter, MinFilter},
};
use image::{buffer::ConvertBuffer, ImageBuffer};
use wgpu::{FilterMode, PrimitiveTopology, TextureFormat};

pub fn component_type_to_index_format(ty: gltf::accessor::DataType) -> wgpu::IndexFormat {
    match ty {
        DataType::U16 => wgpu::IndexFormat::Uint16,
        DataType::U32 => wgpu::IndexFormat::Uint32,
        _ => panic!("Unsupported index format!"),
    }
}

pub fn size_of_component_type(ty: gltf::accessor::DataType) -> usize {
    match ty {
        DataType::I8 => size_of::<u8>(),
        DataType::U8 => size_of::<i8>(),
        DataType::I16 => size_of::<i16>(),
        DataType::U16 => size_of::<u16>(),
        DataType::U32 => size_of::<u32>(),
        DataType::F32 => size_of::<f32>(),
    }
}

pub fn component_count_of_type(dims: Dimensions) -> usize {
    match dims {
        Dimensions::Scalar => 1,
        Dimensions::Vec2 => 2,
        Dimensions::Vec3 => 3,
        Dimensions::Vec4 => 4,
        Dimensions::Mat2 => 4,
        Dimensions::Mat3 => 12,
        Dimensions::Mat4 => 16,
    }
}

pub fn stride_of_component_type(accessor: &gltf::accessor::Accessor) -> usize {
    size_of_component_type(accessor.data_type()) * component_count_of_type(accessor.dimensions())
}

pub fn accessor_type_to_format(accessor: &gltf::accessor::Accessor) -> wgpu::VertexFormat {
    use gltf::accessor::DataType::*;
    use wgpu::VertexFormat::*;
    let normalized = accessor.normalized();
    let dims = accessor.dimensions();
    let ty = accessor.data_type();
    match (normalized, dims, ty) {
        (false, Dimensions::Vec2, U8) => Uint8x2,
        (false, Dimensions::Vec4, U8) => Uint8x4,
        (false, Dimensions::Vec2, I8) => Sint8x2,
        (false, Dimensions::Vec4, I8) => Sint8x4,
        (true, Dimensions::Vec2, U8) => Unorm8x2,
        (true, Dimensions::Vec4, U8) => Unorm8x4,
        (true, Dimensions::Vec2, I8) => Snorm8x2,
        (true, Dimensions::Vec4, I8) => Snorm8x4,
        (false, Dimensions::Vec2, U16) => Uint16x2,
        (false, Dimensions::Vec4, U16) => Uint16x4,
        (false, Dimensions::Vec2, I16) => Sint16x2,
        (false, Dimensions::Vec4, I16) => Sint16x4,
        (true, Dimensions::Vec2, U16) => Unorm16x2,
        (true, Dimensions::Vec4, U16) => Unorm16x4,
        (true, Dimensions::Vec2, I16) => Snorm16x2,
        (true, Dimensions::Vec4, I16) => Snorm16x4,
        (_, Dimensions::Scalar, F32) => Float32,
        (_, Dimensions::Vec2, F32) => Float32x2,
        (_, Dimensions::Vec3, F32) => Float32x3,
        (_, Dimensions::Vec4, F32) => Float32x4,
        (_, Dimensions::Scalar, U32) => Uint32,
        (_, Dimensions::Vec2, U32) => Uint32x2,
        (_, Dimensions::Vec3, U32) => Uint32x3,
        (_, Dimensions::Vec4, U32) => Uint32x4,
        _ => panic!("Unsupported vertex format!"),
    }
}

pub fn convert_sampler(device: &wgpu::Device, sampler: gltf::texture::Sampler) -> wgpu::Sampler {
    let mag_filter = match sampler.mag_filter() {
        Some(MagFilter::Linear) => FilterMode::Nearest,
        Some(MagFilter::Nearest) => FilterMode::Linear,
        None => FilterMode::Linear,
    };
    let min_filter = match sampler.min_filter() {
        Some(
            MinFilter::Linear | MinFilter::LinearMipmapLinear | MinFilter::LinearMipmapNearest,
        ) => FilterMode::Linear,
        _ => FilterMode::Nearest,
    };
    let mipmap_filter = match sampler.min_filter() {
        Some(MinFilter::LinearMipmapLinear | MinFilter::NearestMipmapLinear) => FilterMode::Linear,
        _ => FilterMode::Nearest,
    };

    device.create_sampler(&wgpu::SamplerDescriptor {
        label: sampler.name().or(Some("Unnamed Gltf Sampler")),
        address_mode_u: wrappping_to_address_mode(sampler.wrap_s()),
        address_mode_v: wrappping_to_address_mode(sampler.wrap_t()),
        address_mode_w: wgpu::AddressMode::Repeat,
        mag_filter,
        min_filter,
        mipmap_filter,
        ..Default::default()
    })
}

pub fn convert_image_format(format: gltf::image::Format) -> TextureFormat {
    // TODO: Don't know fitting format for RGB variants
    match format {
        gltf::image::Format::R8 => TextureFormat::R8Unorm,
        gltf::image::Format::R8G8 => TextureFormat::Rg8Unorm,
        gltf::image::Format::R8G8B8 => TextureFormat::Rgba8Unorm,
        gltf::image::Format::R8G8B8A8 => TextureFormat::Rgba8Unorm,
        gltf::image::Format::R16 => TextureFormat::R16Unorm,
        gltf::image::Format::R16G16 => TextureFormat::Rg16Unorm,
        gltf::image::Format::R16G16B16 => TextureFormat::Rgba16Unorm,
        gltf::image::Format::R16G16B16A16 => TextureFormat::Rgba16Unorm,
        gltf::image::Format::R32G32B32FLOAT | gltf::image::Format::R32G32B32A32FLOAT => {
            TextureFormat::Rgba32Float
        }
    }
}

pub fn mesh_mode_to_topology(mode: gltf::mesh::Mode) -> wgpu::PrimitiveTopology {
    use gltf::mesh::Mode;
    use PrimitiveTopology::*;
    match mode {
        Mode::Triangles => TriangleList,
        Mode::TriangleStrip | Mode::TriangleFan => TriangleStrip,
        Mode::Lines => LineList,
        Mode::LineStrip => LineStrip,
        Mode::Points => PointList,
        Mode::LineLoop => todo!("Line Loop!"),
    }
}

pub fn wrappping_to_address_mode(mode: gltf::texture::WrappingMode) -> wgpu::AddressMode {
    use gltf::texture::WrappingMode;
    use wgpu::AddressMode::*;
    match mode {
        WrappingMode::MirroredRepeat => MirrorRepeat,
        WrappingMode::ClampToEdge => ClampToEdge,
        WrappingMode::Repeat => Repeat,
    }
}

type RgbaImage = image::ImageBuffer<image::Rgba<u8>, Vec<u8>>;

pub fn convert_to_rgba(
    image: &gltf::image::Data,
    srgb: bool,
) -> Result<(RgbaImage, wgpu::TextureFormat)> {
    let (width, height) = (image.width, image.height);
    let buf = image.pixels.as_slice();
    let format = image.format;
    let image_image = match format {
        // TODO: convert to R8Unorm
        Format::R8 => ImageBuffer::<image::Luma<u8>, _>::from_raw(width, height, buf)
            .map(|image| image.convert()),
        Format::R8G8 => ImageBuffer::<image::LumaA<u8>, _>::from_raw(width, height, buf)
            .map(|image| image.convert()),
        Format::R8G8B8 => ImageBuffer::<image::Rgb<u8>, _>::from_raw(width, height, buf)
            .map(|image| image.convert()),
        Format::R8G8B8A8 => ImageBuffer::<image::Rgba<u8>, _>::from_raw(width, height, buf)
            .map(|image| image.convert()),
        Format::R16 => {
            ImageBuffer::<image::Luma<u16>, _>::from_raw(width, height, bytemuck::cast_slice(buf))
                .map(|image| image.convert())
        }
        Format::R16G16 => {
            ImageBuffer::<image::LumaA<u16>, _>::from_raw(width, height, bytemuck::cast_slice(buf))
                .map(|image| image.convert())
        }
        Format::R16G16B16 => {
            ImageBuffer::<image::Rgb<u16>, _>::from_raw(width, height, bytemuck::cast_slice(buf))
                .map(|image| image.convert())
        }
        Format::R16G16B16A16 => {
            ImageBuffer::<image::Rgba<u16>, _>::from_raw(width, height, bytemuck::cast_slice(buf))
                .map(|image| image.convert())
        }
        Format::R32G32B32FLOAT => {
            ImageBuffer::<image::Rgb<f32>, _>::from_raw(width, height, bytemuck::cast_slice(buf))
                .map(|image| image.convert())
        }
        Format::R32G32B32A32FLOAT => {
            ImageBuffer::<image::Rgba<f32>, _>::from_raw(width, height, bytemuck::cast_slice(buf))
                .map(|image| image.convert())
        }
    };
    let format = if srgb {
        wgpu::TextureFormat::Rgba8UnormSrgb
    } else {
        wgpu::TextureFormat::Rgba8Unorm
    };
    image_image.map(|img| (img, format)).context(eyre!(
        "Failed to convert {format:?} image with size ({}, {}) to RGBA8",
        width,
        height
    ))
}
