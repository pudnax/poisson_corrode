use crate::{utils::NonZeroSized, FIXED_TIME_STEP};
use std::time::Duration;

use bytemuck::{Pod, Zeroable};
use wgpu::util::DeviceExt;

use super::bind_group_layout::{self, WrappedBindGroupLayout};

pub struct GlobalUniformBinding {
    pub binding: wgpu::BindGroup,
    pub layout: bind_group_layout::BindGroupLayout,
    buffer: wgpu::Buffer,
}

impl GlobalUniformBinding {
    pub const DESC: wgpu::BindGroupLayoutDescriptor<'static> = wgpu::BindGroupLayoutDescriptor {
        label: Some("Global Uniform Bind Group Layout"),
        entries: &[wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::VERTEX_FRAGMENT.union(wgpu::ShaderStages::COMPUTE),
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: Some(Uniform::NSIZE),
            },
            count: None,
        }],
    };

    pub fn new(device: &wgpu::Device) -> Self {
        let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Global Uniform"),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            contents: bytemuck::bytes_of(&Uniform::default()),
        });

        let layout = device.create_bind_group_layout_wrap(&Self::DESC);
        let uniform = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Global Uniform Bind Group"),
            layout: &layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: buffer.as_entire_binding(),
            }],
        });
        Self {
            binding: uniform,
            buffer,
            layout,
        }
    }

    pub fn update(&mut self, queue: &wgpu::Queue, uniform: &Uniform) {
        queue.write_buffer(&self.buffer, 0, bytemuck::bytes_of(uniform))
    }

    pub fn buffer(&self) -> &wgpu::Buffer {
        &self.buffer
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct Uniform {
    pub resolution: [f32; 2],
    pub frame: u32,
    pub time: f32,
    pub dt: f32,
    pub custom: f32,
    pub prev_jitter: [f32; 2],
    pub jitter: [f32; 2],
}

impl Default for Uniform {
    fn default() -> Self {
        Self {
            time: 0.,
            resolution: [1920.0, 780.],
            frame: 0,
            dt: FIXED_TIME_STEP as _,
            custom: 0.,
            prev_jitter: [0.; 2],
            jitter: [0.; 2],
        }
    }
}

impl std::fmt::Display for Uniform {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let time = Duration::from_secs_f32(self.time);
        write!(
            f,
            "time:\t\t{:#.2?}\n\
              width, height:\t{:?}\n\
              frame:\t\t{}\n",
            time, self.resolution, self.frame,
        )
    }
}
