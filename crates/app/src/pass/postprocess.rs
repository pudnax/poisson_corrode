use crate::{
    pipeline::{PipelineArena, RenderHandle, RenderPipelineDescriptor},
    GlobalUniformBinding, ProfilerCommandEncoder, ViewTarget, WrappedBindGroupLayout,
    DEFAULT_SAMPLER_DESC,
};
use eyre::Result;
use components::{bind_group_layout::SingleTextureBindGroupLayout, world::World};
use std::path::Path;

use super::Pass;

pub struct PostProcess {
    pipeline: RenderHandle,
    sampler: wgpu::BindGroup,
}

impl PostProcess {
    pub fn new(world: &World, path: impl AsRef<Path>) -> Result<Self> {
        let global_ubo = world.get::<GlobalUniformBinding>()?;
        let mut pipeline_arena = world.get_mut::<PipelineArena>()?;
        let texture_bind_group_layout = world.unwrap::<SingleTextureBindGroupLayout>();

        let sampler = world.device().create_sampler(&DEFAULT_SAMPLER_DESC);
        let sampler_bind_group_layout =
            world
                .device()
                .create_bind_group_layout_wrap(&wgpu::BindGroupLayoutDescriptor {
                    label: Some("Post Process Bind Group Layout"),
                    entries: &[wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::VERTEX_FRAGMENT
                            | wgpu::ShaderStages::COMPUTE,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    }],
                });
        let sampler = world
            .device()
            .create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("Blit Sampler Bind Group"),
                layout: &sampler_bind_group_layout,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                }],
            });

        let desc = RenderPipelineDescriptor {
            label: Some("Post Process Pipeline".into()),
            layout: vec![
                global_ubo.layout.clone(),
                texture_bind_group_layout.layout.clone(),
                sampler_bind_group_layout,
            ],
            depth_stencil: None,
            ..Default::default()
        };
        let pipeline = pipeline_arena.process_render_pipeline_from_path(path, desc)?;
        Ok(Self { pipeline, sampler })
    }
}

pub struct PostProcessResource<'a> {
    pub view_target: &'a ViewTarget,
}

impl Pass for PostProcess {
    type Resources<'a> = PostProcessResource<'a>;

    fn record(
        &self,
        world: &World,
        encoder: &mut ProfilerCommandEncoder,
        resource: Self::Resources<'_>,
    ) {
        let global_ubo = world.unwrap::<GlobalUniformBinding>();
        let post_process_target = resource.view_target.post_process_write();
        let arena = world.unwrap::<PipelineArena>();

        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Post Process Pass"),
            color_attachments: &[Some(post_process_target.get_color_attachment(
                wgpu::Color {
                    r: 0.,
                    g: 0.,
                    b: 0.,
                    a: 0.0,
                },
            ))],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        pass.set_bind_group(0, &global_ubo.binding, &[]);
        pass.set_bind_group(1, post_process_target.source_binding, &[]);
        pass.set_bind_group(2, &self.sampler, &[]);
        pass.set_pipeline(arena.get_pipeline(self.pipeline));
        pass.draw(0..3, 0..1);
    }
}
