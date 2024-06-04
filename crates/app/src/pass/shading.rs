use std::path::Path;

use eyre::Result;
use pools::MeshPool;

use crate::{
    pipeline::{PipelineArena, RenderHandle, RenderPipelineDescriptor},
    GBuffer, GlobalsBindGroup, ProfilerCommandEncoder, ViewTarget,
    {LightPool, MaterialPool, TexturePool},
};
use components::world::World;

use super::Pass;

pub struct ShadingPass {
    pipeline: RenderHandle,
}

impl ShadingPass {
    pub fn new(shader: impl AsRef<Path>, world: &World, gbuffer: &GBuffer) -> Result<Self> {
        let globals = world.get::<GlobalsBindGroup>()?;
        let materials = world.get::<MaterialPool>()?;
        let textures = world.get::<TexturePool>()?;
        let lights = world.get::<LightPool>()?;
        let meshes = world.get::<MeshPool>()?;
        let desc = RenderPipelineDescriptor {
            label: Some("Shading Pipeline".into()),
            layout: vec![
                globals.layout.clone(),
                gbuffer.bind_group_layout.clone(),
                textures.bind_group_layout.clone(),
                materials.bind_group_layout.clone(),
                lights.point_bind_group_layout.clone(),
                lights.area_bind_group_layout.clone(),
                meshes.trace_bind_group_layout.clone(),
            ],
            depth_stencil: None,
            ..Default::default()
        };
        let pipeline = world
            .get_mut::<PipelineArena>()?
            .process_render_pipeline_from_path(shader, desc)?;
        Ok(Self { pipeline })
    }
}

pub struct ShadingResource<'a> {
    pub gbuffer: &'a GBuffer,
    pub view_target: &'a ViewTarget,
}

impl Pass for ShadingPass {
    type Resources<'a> = ShadingResource<'a>;

    fn record(
        &self,
        world: &World,
        encoder: &mut ProfilerCommandEncoder,
        resources: Self::Resources<'_>,
    ) {
        let globals = world.unwrap::<GlobalsBindGroup>();
        let arena = world.unwrap::<PipelineArena>();
        let textures = world.unwrap::<TexturePool>();
        let materials = world.unwrap::<MaterialPool>();
        let lights = world.unwrap::<LightPool>();
        let meshes = world.unwrap::<MeshPool>();

        let mut rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Shading Pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: resources.view_target.main_view(),
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });

        rpass.set_pipeline(arena.get_pipeline(self.pipeline));
        rpass.set_bind_group(0, &globals.binding, &[]);
        rpass.set_bind_group(1, &resources.gbuffer.bind_group, &[]);
        rpass.set_bind_group(2, &textures.bind_group, &[]);
        rpass.set_bind_group(3, &materials.bind_group, &[]);
        rpass.set_bind_group(4, &lights.point_bind_group, &[]);
        rpass.set_bind_group(5, &lights.area_bind_group, &[]);
        rpass.set_bind_group(6, &meshes.trace_bind_group, &[]);

        rpass.draw(0..3, 0..1);
    }
}
