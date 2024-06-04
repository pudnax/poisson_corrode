mod boxx;
mod cube;
mod plane;
mod sphere;

use core::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use glam::{Vec2, Vec3, Vec4};

use components::bind_group_layout::{self, WrappedBindGroupLayout};
use components::{BindGroupLayout, Gpu, Instance, MeshId, MeshInfo};
use components::{NonZeroSized, ResizableBuffer, ResizableBufferExt};

use bvh::{BvhBuilder, BvhNode, Tlas, TlasNode};

pub use boxx::make_box_mesh;
pub use cube::make_cube_mesh;
pub use plane::make_plane_mesh;
pub use sphere::make_uv_sphere;

pub fn calculate_bounds(positions: &[Vec3]) -> (Vec3, Vec3) {
    positions.iter().fold(
        (Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY)),
        |(min, max), &pos| (min.min(pos), max.max(pos)),
    )
}

pub struct Mesh {
    pub vertices: Vec<Vec3>,
    pub normals: Vec<Vec3>,
    pub tangents: Vec<Vec4>,
    pub tex_coords: Vec<Vec2>,
    pub indices: Vec<u32>,
}

impl Mesh {
    pub fn as_ref(&self) -> MeshRef {
        MeshRef {
            vertices: &self.vertices,
            normals: &self.normals,
            tangents: &self.tangents,
            tex_coords: &self.tex_coords,
            indices: self.indices.to_vec(),
        }
    }
}

pub struct MeshRef<'a> {
    pub vertices: &'a [Vec3],
    pub normals: &'a [Vec3],
    pub tangents: &'a [Vec4],
    pub tex_coords: &'a [Vec2],
    pub indices: Vec<u32>,
}

pub struct MeshPool {
    vertex_offset: AtomicU32,
    base_index: AtomicU32,
    mesh_index: AtomicU32,
    bvh_index: AtomicU32,

    pub mesh_info_layout: bind_group_layout::BindGroupLayout,
    pub mesh_info_bind_group: wgpu::BindGroup,
    pub mesh_info_cpu: Vec<MeshInfo>,
    pub mesh_info: ResizableBuffer<MeshInfo>,

    pub vertices: ResizableBuffer<Vec3>,
    pub normals: ResizableBuffer<Vec3>,
    pub tangents: ResizableBuffer<Vec4>,
    pub tex_coords: ResizableBuffer<Vec2>,
    pub indices: ResizableBuffer<u32>,
    pub bvh_nodes: ResizableBuffer<BvhNode>,

    pub tlas: Tlas,
    pub tlas_nodes: ResizableBuffer<TlasNode>,

    pub trace_bind_group_layout: BindGroupLayout,
    pub trace_bind_group: wgpu::BindGroup,

    gpu: Arc<Gpu>,
}

impl MeshPool {
    pub const HORIZONTAL_PLANE_MESH: MeshId = MeshId::new(0);
    pub const VERTICAL_PLANE_MESH: MeshId = MeshId::new(1);
    pub const SPHERE_1_MESH: MeshId = MeshId::new(2);
    pub const SPHERE_10_MESH: MeshId = MeshId::new(3);

    pub fn new(gpu: Arc<Gpu>) -> Self {
        let vertices = gpu
            .device()
            .create_resizable_buffer(wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::STORAGE);
        let normals = gpu
            .device()
            .create_resizable_buffer(wgpu::BufferUsages::VERTEX);
        let tangents = gpu
            .device()
            .create_resizable_buffer(wgpu::BufferUsages::VERTEX);
        let tex_coords = gpu
            .device()
            .create_resizable_buffer(wgpu::BufferUsages::VERTEX);
        let indices = gpu
            .device()
            .create_resizable_buffer(wgpu::BufferUsages::INDEX | wgpu::BufferUsages::STORAGE);
        let bvh_nodes = gpu
            .device()
            .create_resizable_buffer(wgpu::BufferUsages::STORAGE);
        let tlas = Tlas::empty();
        let tlas_nodes = gpu
            .device()
            .create_resizable_buffer(wgpu::BufferUsages::STORAGE);

        let mesh_info = gpu
            .device()
            .create_resizable_buffer(wgpu::BufferUsages::STORAGE);
        let mesh_info_layout =
            gpu.device()
                .create_bind_group_layout_wrap(&wgpu::BindGroupLayoutDescriptor {
                    label: Some("Mesh Info Bind Group Layout"),
                    entries: &[wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::COMPUTE
                            | wgpu::ShaderStages::VERTEX_FRAGMENT,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Storage { read_only: true },
                            has_dynamic_offset: false,
                            min_binding_size: Some(MeshInfo::NSIZE),
                        },
                        count: None,
                    }],
                });
        let mesh_info_bind_group =
            Self::mesh_info_bind_group(gpu.device(), &mesh_info_layout, &mesh_info);

        let trace_bind_group_layout =
            gpu.device()
                .create_bind_group_layout_wrap(&wgpu::BindGroupLayoutDescriptor {
                    label: Some("Trace BGL"),
                    entries: &[
                        wgpu::BindGroupLayoutEntry {
                            binding: 0,
                            visibility: wgpu::ShaderStages::FRAGMENT,
                            ty: wgpu::BindingType::Buffer {
                                ty: wgpu::BufferBindingType::Storage { read_only: true },
                                has_dynamic_offset: false,
                                min_binding_size: Some(TlasNode::NSIZE),
                            },
                            count: None,
                        },
                        wgpu::BindGroupLayoutEntry {
                            binding: 1,
                            visibility: wgpu::ShaderStages::FRAGMENT,
                            ty: wgpu::BindingType::Buffer {
                                ty: wgpu::BufferBindingType::Storage { read_only: true },
                                has_dynamic_offset: false,
                                min_binding_size: Some(Instance::NSIZE),
                            },
                            count: None,
                        },
                        wgpu::BindGroupLayoutEntry {
                            binding: 2,
                            visibility: wgpu::ShaderStages::FRAGMENT,
                            ty: wgpu::BindingType::Buffer {
                                ty: wgpu::BufferBindingType::Storage { read_only: true },
                                has_dynamic_offset: false,
                                min_binding_size: Some(MeshInfo::NSIZE),
                            },
                            count: None,
                        },
                        wgpu::BindGroupLayoutEntry {
                            binding: 3,
                            visibility: wgpu::ShaderStages::FRAGMENT,
                            ty: wgpu::BindingType::Buffer {
                                ty: wgpu::BufferBindingType::Storage { read_only: true },
                                has_dynamic_offset: false,
                                min_binding_size: Some(BvhNode::NSIZE),
                            },
                            count: None,
                        },
                        wgpu::BindGroupLayoutEntry {
                            binding: 4,
                            visibility: wgpu::ShaderStages::FRAGMENT,
                            ty: wgpu::BindingType::Buffer {
                                ty: wgpu::BufferBindingType::Storage { read_only: true },
                                has_dynamic_offset: false,
                                min_binding_size: Some(f32::NSIZE),
                            },
                            count: None,
                        },
                        wgpu::BindGroupLayoutEntry {
                            binding: 5,
                            visibility: wgpu::ShaderStages::FRAGMENT,
                            ty: wgpu::BindingType::Buffer {
                                ty: wgpu::BufferBindingType::Storage { read_only: true },
                                has_dynamic_offset: false,
                                min_binding_size: Some(u32::NSIZE),
                            },
                            count: None,
                        },
                    ],
                });

        let trace_bind_group = {
            let instances = gpu
                .device()
                .create_resizable_buffer::<Instance>(wgpu::BufferUsages::STORAGE);
            gpu.device().create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("Trace BG"),
                layout: &trace_bind_group_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: tlas_nodes.as_tight_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: instances.as_tight_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: mesh_info.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: bvh_nodes.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 4,
                        resource: vertices.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 5,
                        resource: indices.as_entire_binding(),
                    },
                ],
            })
        };

        let mut this = Self {
            vertex_offset: AtomicU32::new(0),
            base_index: AtomicU32::new(0),
            mesh_index: AtomicU32::new(0),
            bvh_index: AtomicU32::new(0),

            mesh_info_layout,
            mesh_info_bind_group,
            mesh_info_cpu: vec![],
            mesh_info,

            vertices,
            indices,
            normals,
            tangents,
            tex_coords,
            bvh_nodes,

            tlas,
            tlas_nodes,

            trace_bind_group_layout,
            trace_bind_group,

            gpu,
        };

        let mut plane_mesh = make_plane_mesh(1., 1.);
        this.add(plane_mesh.as_ref());
        let rot = glam::Mat3::from_rotation_x(-std::f32::consts::PI / 2.);
        plane_mesh.vertices.iter_mut().for_each(|v| *v = rot * *v);
        plane_mesh.normals.iter_mut().for_each(|v| *v = rot * *v);
        this.add(plane_mesh.as_ref());
        this.add(make_uv_sphere(1., 1).as_ref());
        this.add(make_uv_sphere(1., 10).as_ref());

        this
    }

    pub fn generate_tlas(&mut self, instances: &[Instance]) {
        if instances.is_empty() {
            return;
        }
        self.tlas_nodes.clear();
        self.tlas.build(instances, &self.mesh_info_cpu);
        self.tlas_nodes.push(&self.gpu, &self.tlas.nodes);
    }

    pub fn mesh_info_bind_group(
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        mesh_info: &ResizableBuffer<MeshInfo>,
    ) -> wgpu::BindGroup {
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Mesh Info Bind Group"),
            layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: mesh_info.as_tight_binding(),
            }],
        });

        bind_group
    }

    pub fn count(&self) -> u32 {
        self.mesh_index.load(Ordering::Relaxed)
    }

    pub fn add(&mut self, mut mesh: MeshRef) -> MeshId {
        let vertex_count = mesh.vertices.len() as u32;
        let vertex_offset = self
            .vertex_offset
            .fetch_add(vertex_count, Ordering::Relaxed);

        self.vertices.push(&self.gpu, mesh.vertices);
        self.normals.push(&self.gpu, mesh.normals);
        self.tangents.push(&self.gpu, mesh.tangents);
        self.tex_coords.push(&self.gpu, mesh.tex_coords);

        let bvh =
            BvhBuilder::new(mesh.vertices, bytemuck::cast_slice_mut(&mut mesh.indices)).build();
        let bvh_index = self
            .bvh_index
            .fetch_add(bvh.nodes.len() as u32, Ordering::Relaxed);
        self.bvh_nodes.push(&self.gpu, &bvh.nodes);

        let index_count = mesh.indices.len() as u32;
        let base_index = self.base_index.fetch_add(index_count, Ordering::Relaxed);

        self.indices.push(&self.gpu, &mesh.indices);
        let mesh_index = self.mesh_index.fetch_add(1, Ordering::Relaxed);

        let (min, max) = calculate_bounds(mesh.vertices);

        let mesh_info = MeshInfo {
            min,
            vertex_offset: vertex_offset as i32,
            max,
            base_index,
            index_count,
            bvh_index,
            junk: [0; 2],
        };
        self.mesh_info_cpu.push(mesh_info);
        self.mesh_info.push(&self.gpu, &[mesh_info]);
        self.mesh_info_bind_group =
            Self::mesh_info_bind_group(self.gpu.device(), &self.mesh_info_layout, &self.mesh_info);

        log::info!("Added new mesh with id: {mesh_index}");
        MeshId(mesh_index)
    }
}
