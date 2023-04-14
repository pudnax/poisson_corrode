use std::sync::Arc;

use glam::Mat4;

use crate::{
    utils::{NonZeroSized, ResizableBuffer, ResizableBufferExt},
    Gpu,
};

use super::{
    bind_group_layout::{self, WrappedBindGroupLayout},
    material::MaterialId,
    mesh::MeshId,
};

#[repr(C)]
#[derive(Debug, Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Instance {
    pub transform: glam::Mat4,
    pub mesh: MeshId,
    pub material: MaterialId,
    pub junk: [u32; 2],
}

impl Default for Instance {
    fn default() -> Self {
        Self {
            transform: Mat4::IDENTITY,
            mesh: MeshId::default(),
            material: MaterialId::default(),
            junk: [0; 2],
        }
    }
}

impl Instance {
    pub fn new(transform: glam::Mat4, mesh: MeshId, material: MaterialId) -> Self {
        Self {
            transform,
            mesh,
            material,
            junk: [0; 2],
        }
    }

    pub fn transform(&mut self, transform: glam::Mat4) {
        self.transform = transform * self.transform;
    }
}

pub struct InstancesManager {
    instances_data: Vec<Instance>,
    pub(crate) instances: ResizableBuffer<Instance>,

    pub bind_group: wgpu::BindGroup,
    pub bind_group_layout: bind_group_layout::BindGroupLayout,
    gpu: Arc<Gpu>,
}

impl InstancesManager {
    pub const MAX_INSTANCES: usize = 1_000_000;
    const LAYOUT: wgpu::BindGroupLayoutDescriptor<'static> = wgpu::BindGroupLayoutDescriptor {
        label: Some("Draw Instances Bind Group Layout"),
        entries: &[wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::COMPUTE.union(wgpu::ShaderStages::VERTEX_FRAGMENT),
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage { read_only: true },
                has_dynamic_offset: false,
                min_binding_size: Some(Instance::NSIZE),
            },
            count: None,
        }],
    };

    pub fn new(gpu: Arc<Gpu>) -> Self {
        let instances_data = Vec::with_capacity(32);
        let instances = gpu.device().create_resizable_buffer(
            wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::VERTEX,
        );

        let bind_group_layout = gpu.device.create_bind_group_layout_wrap(&Self::LAYOUT);
        let bind_group = Self::create_bind_group(gpu.device(), &bind_group_layout, &instances);

        Self {
            instances_data,
            instances,
            bind_group,
            bind_group_layout,
            gpu,
        }
    }

    pub fn create_bind_group(
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        instances: &ResizableBuffer<Instance>,
    ) -> wgpu::BindGroup {
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Draw Instances Bind Group"),
            layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: instances.as_entire_binding(),
            }],
        });

        bind_group
    }

    pub fn add(&mut self, instances: &[Instance]) {
        self.instances_data.extend_from_slice(instances);
        if self.instances.push(&self.gpu, instances) {
            let bind_group = Self::create_bind_group(
                self.gpu.device(),
                &self.bind_group_layout,
                &self.instances,
            );
            self.bind_group = bind_group;
        }
    }

    pub fn count(&self) -> u32 {
        self.instances.len() as _
    }

    pub fn clear(&mut self) {
        self.instances_data.clear();
        self.instances.clear();
    }
}