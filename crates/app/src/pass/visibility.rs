use std::path::Path;

use eyre::Result;
use components::bind_group_layout::StorageWriteBindGroupLayout;
use components::world::World;
use components::{DrawIndexedIndirect, NonZeroSized, ResizableBuffer};
use glam::{Vec2, Vec3, Vec4};
use wgpu::{util::align_to, IndexFormat};

use super::Pass;

use crate::ProfilerCommandEncoder;
use crate::{
    pipeline::{
        self, ComputeHandle, ComputePipelineDescriptor, PipelineArena, RenderHandle,
        RenderPipelineDescriptor,
    },
    CameraUniformBinding, GBuffer, InstancePool, MaterialPool, MeshPool, TexturePool,
};

pub struct Visibility {
    geometry: Geometry,
    emit_draws: EmitDraws,
}

impl Visibility {
    pub fn new(world: &World) -> Result<Self> {
        Ok(Self {
            geometry: Geometry::new(world)?,
            emit_draws: EmitDraws::new(world)?,
        })
    }
}

pub struct VisibilityResource<'a> {
    pub gbuffer: &'a GBuffer,

    pub draw_cmd_buffer: &'a ResizableBuffer<DrawIndexedIndirect>,
    pub draw_cmd_bind_group: &'a wgpu::BindGroup,
}

impl Pass for Visibility {
    type Resources<'a> = VisibilityResource<'a>;
    fn record(
        &self,
        world: &World,
        encoder: &mut ProfilerCommandEncoder,
        resources: Self::Resources<'_>,
    ) {
        encoder.profile_start("Visibility");
        self.emit_draws.record(
            world,
            encoder,
            EmitDrawsResource {
                draw_cmd_buffer: resources.draw_cmd_buffer,
                draw_cmd_bind_group: resources.draw_cmd_bind_group,
            },
        );
        self.geometry.record(
            world,
            encoder,
            GeometryResource {
                gbuffer: resources.gbuffer,
                draw_cmd_buffer: resources.draw_cmd_buffer,
            },
        );
        encoder.profile_end();
    }
}

struct Geometry {
    pipeline: RenderHandle,
}

impl Geometry {
    pub fn new(world: &World) -> Result<Self> {
        let path = Path::new("shaders").join("visibility.wgsl");
        let textures = world.get::<TexturePool>()?;
        let materials = world.get::<MaterialPool>()?;
        let instances = world.get::<InstancePool>()?;
        let camera = world.get::<CameraUniformBinding>()?;
        let render_desc = RenderPipelineDescriptor {
            label: Some("Visibilty Pipeline".into()),
            layout: vec![
                camera.bind_group_layout.clone(),
                textures.bind_group_layout.clone(),
                instances.bind_group_layout.clone(),
                materials.bind_group_layout.clone(),
            ],
            vertex: pipeline::VertexState {
                entry_point: "vs_main".into(),
                buffers: vec![
                    // Positions
                    pipeline::VertexBufferLayout {
                        array_stride: Vec3::SIZE as _,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: wgpu::vertex_attr_array![0 => Float32x3].to_vec(),
                    },
                    // Normals
                    pipeline::VertexBufferLayout {
                        array_stride: Vec3::SIZE as _,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: wgpu::vertex_attr_array![1 => Float32x3].to_vec(),
                    },
                    // Tangents
                    pipeline::VertexBufferLayout {
                        array_stride: Vec4::SIZE as _,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: wgpu::vertex_attr_array![2 => Float32x4].to_vec(),
                    },
                    // UVs
                    pipeline::VertexBufferLayout {
                        array_stride: Vec2::SIZE as _,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: wgpu::vertex_attr_array![3 => Float32x2].to_vec(),
                    },
                ],
            },
            fragment: Some(pipeline::FragmentState {
                entry_point: "fs_main".into(),
                targets: GBuffer::color_target_state().into(),
            }),
            primitive: wgpu::PrimitiveState {
                cull_mode: Some(wgpu::Face::Back),
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: GBuffer::DEPTH_FORMAT,
                depth_write_enabled: true,
                depth_compare: wgpu::CompareFunction::Greater,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            ..Default::default()
        };
        let pipeline = world
            .get_mut::<PipelineArena>()?
            .process_render_pipeline_from_path(path, render_desc)?;
        Ok(Self { pipeline })
    }
}

struct GeometryResource<'a> {
    pub gbuffer: &'a GBuffer,

    pub draw_cmd_buffer: &'a ResizableBuffer<DrawIndexedIndirect>,
}

impl Pass for Geometry {
    type Resources<'a> = GeometryResource<'a>;
    fn record(
        &self,
        world: &World,
        encoder: &mut ProfilerCommandEncoder,
        resources: Self::Resources<'_>,
    ) {
        let meshes = world.unwrap::<MeshPool>();
        let textures = world.unwrap::<TexturePool>();
        let materials = world.unwrap::<MaterialPool>();
        let instances = world.unwrap::<InstancePool>();
        let arena = world.unwrap::<PipelineArena>();
        let camera = world.unwrap::<CameraUniformBinding>();

        let mut rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Visibility Pass"),
            color_attachments: &resources.gbuffer.color_target_attachment(),
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &resources.gbuffer.depth,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(0.0),
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
        });

        rpass.set_pipeline(arena.get_pipeline(self.pipeline));
        rpass.set_bind_group(0, &camera.binding, &[]);
        rpass.set_bind_group(1, &textures.bind_group, &[]);
        rpass.set_bind_group(2, &instances.bind_group, &[]);
        rpass.set_bind_group(3, &materials.bind_group, &[]);

        rpass.set_vertex_buffer(0, meshes.vertices.full_slice());
        rpass.set_vertex_buffer(1, meshes.normals.full_slice());
        rpass.set_vertex_buffer(2, meshes.tangents.full_slice());
        rpass.set_vertex_buffer(3, meshes.tex_coords.full_slice());
        rpass.set_index_buffer(meshes.indices.full_slice(), IndexFormat::Uint32);
        rpass.multi_draw_indexed_indirect(
            resources.draw_cmd_buffer,
            0,
            resources.draw_cmd_buffer.len() as _,
        );
    }
}

struct EmitDraws {
    pipeline: ComputeHandle,
}

impl EmitDraws {
    pub fn new(world: &World) -> Result<Self> {
        let camera = world.get::<CameraUniformBinding>()?;
        let meshes = world.get::<MeshPool>()?;
        let instances = world.get::<InstancePool>()?;
        let draw_cmd_layout = world.get::<StorageWriteBindGroupLayout<DrawIndexedIndirect>>()?;
        let path = Path::new("shaders").join("emit_draws.wgsl");
        let comp_desc = ComputePipelineDescriptor {
            label: Some("Emit Draws Pipeline".into()),
            layout: vec![
                camera.bind_group_layout.clone(),
                meshes.mesh_info_layout.clone(),
                instances.bind_group_layout.clone(),
                draw_cmd_layout.layout.clone(),
            ],
            push_constant_ranges: vec![],
            entry_point: "emit_draws".into(),
        };
        let pipeline = world
            .get_mut::<PipelineArena>()?
            .process_compute_pipeline_from_path(path, comp_desc)?;
        Ok(Self { pipeline })
    }
}

struct EmitDrawsResource<'a> {
    pub draw_cmd_bind_group: &'a wgpu::BindGroup,
    pub draw_cmd_buffer: &'a ResizableBuffer<DrawIndexedIndirect>,
}

impl Pass for EmitDraws {
    type Resources<'a> = EmitDrawsResource<'a>;

    fn record(
        &self,
        world: &World,
        encoder: &mut ProfilerCommandEncoder,
        resources: Self::Resources<'_>,
    ) {
        let camera = world.unwrap::<CameraUniformBinding>();
        let meshes = world.unwrap::<MeshPool>();
        let arena = world.unwrap::<PipelineArena>();
        let instances = world.unwrap::<InstancePool>();
        let mut cpass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("Emit Draws Pass"),
            timestamp_writes: None,
        });

        cpass.set_pipeline(arena.get_pipeline(self.pipeline));
        cpass.set_bind_group(0, &camera.binding, &[]);
        cpass.set_bind_group(1, &meshes.mesh_info_bind_group, &[]);
        cpass.set_bind_group(2, &instances.bind_group, &[]);
        cpass.set_bind_group(3, resources.draw_cmd_bind_group, &[]);
        let num_dispatches = align_to(resources.draw_cmd_buffer.len() as _, 64) / 64;
        cpass.dispatch_workgroups(num_dispatches, 1, 1);
    }
}
