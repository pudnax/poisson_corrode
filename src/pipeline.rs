use std::{
    borrow::Cow,
    collections::{hash_map::Entry, HashMap},
    num::NonZeroU32,
    path::{Path, PathBuf},
};

use either::Either;
use slotmap::{SecondaryMap, SlotMap};
use wgpu::{
    BufferAddress, ColorTargetState, DepthStencilState, MultisampleState, PrimitiveState,
    PushConstantRange, VertexAttribute, VertexFormat, VertexStepMode,
};

use crate::{app::App, bind_group_layout, watcher::Watcher};

slotmap::new_key_type! {
    pub struct RenderHandle;
    pub struct ComputeHandle;
}

pub struct Arena {
    render: RenderArena,
    compute: ComputeArena,
    path_mapping: HashMap<PathBuf, Vec<Either<RenderHandle, ComputeHandle>>>,
    file_watcher: Watcher,
}

struct RenderArena {
    pipelines: SlotMap<RenderHandle, wgpu::RenderPipeline>,
    descriptors: SecondaryMap<RenderHandle, RenderPipelineDescriptor>,
    cached: HashMap<RenderPipelineDescriptor, RenderHandle>,
}

impl RenderArena {
    fn process_pipeline(
        &mut self,
        device: &wgpu::Device,
        module: &wgpu::ShaderModule,
        descriptor: RenderPipelineDescriptor,
    ) -> RenderHandle {
        match self.cached.entry(descriptor.clone()) {
            Entry::Occupied(entry) => *entry.get(),
            Entry::Vacant(entry) => {
                let pipeline = entry.key().process(device, module);
                let handle = self.pipelines.insert(pipeline);
                self.descriptors.insert(handle, descriptor);
                entry.insert(handle);
                handle
            }
        }
    }

    fn reload_pipeline(
        &mut self,
        device: &wgpu::Device,
        module: &wgpu::ShaderModule,
        handle: RenderHandle,
    ) {
        let desc = &self.descriptors[handle];
        self.pipelines[handle] = desc.process(device, module);
    }
}

struct ComputeArena {
    pipelines: SlotMap<ComputeHandle, wgpu::ComputePipeline>,
    descriptors: SecondaryMap<ComputeHandle, ComputePipelineDescriptor>,
    cached: HashMap<ComputePipelineDescriptor, ComputeHandle>,
}

impl ComputeArena {
    fn process_pipeline(
        &mut self,
        device: &wgpu::Device,
        module: &wgpu::ShaderModule,
        descriptor: ComputePipelineDescriptor,
    ) -> ComputeHandle {
        match self.cached.entry(descriptor.clone()) {
            Entry::Occupied(entry) => *entry.get(),
            Entry::Vacant(entry) => {
                let pipeline = entry.key().process(device, module);
                let handle = self.pipelines.insert(pipeline);
                self.descriptors.insert(handle, descriptor);
                entry.insert(handle);
                handle
            }
        }
    }

    fn reload_pipeline(
        &mut self,
        device: &wgpu::Device,
        module: &wgpu::ShaderModule,
        handle: ComputeHandle,
    ) {
        let desc = &self.descriptors[handle];
        self.pipelines[handle] = desc.process(device, module);
    }
}

pub trait Handle {
    type Pipeline;
    type Descriptor;
    fn get_pipeline(self, arena: &Arena) -> &Self::Pipeline;
    fn get_descriptor(self, arena: &Arena) -> &Self::Descriptor;
}

impl Handle for RenderHandle {
    type Pipeline = wgpu::RenderPipeline;
    type Descriptor = RenderPipelineDescriptor;

    fn get_pipeline(self, arena: &Arena) -> &Self::Pipeline {
        &arena.render.pipelines[self]
    }

    fn get_descriptor(self, arena: &Arena) -> &Self::Descriptor {
        &arena.render.descriptors[self]
    }
}

impl Handle for ComputeHandle {
    type Pipeline = wgpu::ComputePipeline;
    type Descriptor = ComputePipelineDescriptor;
    fn get_pipeline(self, arena: &Arena) -> &Self::Pipeline {
        &arena.compute.pipelines[self]
    }

    fn get_descriptor(self, arena: &Arena) -> &Self::Descriptor {
        &arena.compute.descriptors[self]
    }
}

impl Arena {
    pub fn new(file_watcher: Watcher) -> Self {
        Self {
            render: RenderArena {
                pipelines: SlotMap::with_key(),
                descriptors: SecondaryMap::new(),
                cached: HashMap::new(),
            },
            compute: ComputeArena {
                pipelines: SlotMap::with_key(),
                descriptors: SecondaryMap::new(),
                cached: HashMap::new(),
            },
            path_mapping: HashMap::new(),
            file_watcher,
        }
    }

    pub fn get_pipeline<H: Handle>(&self, handle: H) -> &H::Pipeline {
        handle.get_pipeline(self)
    }

    pub fn get_descriptor<H: Handle>(&self, handle: H) -> &H::Descriptor {
        handle.get_descriptor(self)
    }

    pub fn process_render_pipeline_from_path(
        &mut self,
        device: &wgpu::Device,
        path: impl AsRef<Path>,
        descriptor: RenderPipelineDescriptor,
    ) -> RenderHandle {
        let module = crate::utils::create_shader_module_with_path(device, path.as_ref());
        self.process_render_pipeline(device, path.as_ref(), &module, descriptor)
    }

    pub fn process_render_pipeline(
        &mut self,
        device: &wgpu::Device,
        path: &Path,
        module: &wgpu::ShaderModule,
        descriptor: RenderPipelineDescriptor,
    ) -> RenderHandle {
        self.file_watcher.watch_file(path).unwrap();
        let handle = self.render.process_pipeline(device, module, descriptor);
        self.path_mapping
            .entry(path.to_path_buf())
            .or_default()
            .push(Either::Left(handle));
        handle
    }

    pub fn process_compute_pipeline_from_path(
        &mut self,
        device: &wgpu::Device,
        path: impl AsRef<Path>,
        descriptor: ComputePipelineDescriptor,
    ) -> ComputeHandle {
        let module = crate::utils::create_shader_module_with_path(device, path.as_ref());
        self.process_compute_pipeline(device, path.as_ref(), &module, descriptor)
    }

    pub fn process_compute_pipeline(
        &mut self,
        device: &wgpu::Device,
        path: &Path,
        module: &wgpu::ShaderModule,
        descriptor: ComputePipelineDescriptor,
    ) -> ComputeHandle {
        self.file_watcher.watch_file(path).unwrap();
        let handle = self.compute.process_pipeline(device, module, descriptor);
        self.path_mapping
            .entry(path.to_path_buf())
            .or_default()
            .push(Either::Right(handle));
        handle
    }

    pub fn reload_pipelines(
        &mut self,
        device: &wgpu::Device,
        path: &Path,
        module: &wgpu::ShaderModule,
    ) {
        for handle in &self.path_mapping[path] {
            match handle {
                Either::Left(handle) => self.render.reload_pipeline(device, module, *handle),
                Either::Right(handle) => self.compute.reload_pipeline(device, module, *handle),
            }
        }
    }
}

/// Describes render pipeline.
#[derive(Debug, Hash, PartialEq, Eq, Clone)]
pub struct RenderPipelineDescriptor {
    pub label: Option<Cow<'static, str>>,
    pub layout: Vec<bind_group_layout::BindGroupLayout>,
    pub push_constant_ranges: Vec<PushConstantRange>,
    pub vertex: VertexState,
    pub primitive: PrimitiveState,
    pub depth_stencil: Option<DepthStencilState>,
    pub multisample: MultisampleState,
    pub fragment: Option<FragmentState>,
    pub multiview: Option<NonZeroU32>,
}

impl RenderPipelineDescriptor {
    pub fn name(&self) -> &str {
        self.label
            .as_ref()
            .map(|name| name.as_ref())
            .unwrap_or("Render Pipeline")
    }

    pub fn process(
        &self,
        device: &wgpu::Device,
        module: &wgpu::ShaderModule,
    ) -> wgpu::RenderPipeline {
        let vertex_buffer_layouts = self
            .vertex
            .buffers
            .iter()
            .map(|layout| wgpu::VertexBufferLayout {
                array_stride: layout.array_stride,
                attributes: &layout.attributes,
                step_mode: layout.step_mode,
            })
            .collect::<Vec<_>>();

        let bind_group_layouts = self.layout.iter().map(|x| x.value()).collect::<Vec<_>>();
        let layout = if self.push_constant_ranges.is_empty() && self.layout.is_empty() {
            None
        } else {
            Some(
                device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                    label: self.label.as_deref(),
                    push_constant_ranges: &self.push_constant_ranges,
                    bind_group_layouts: &bind_group_layouts,
                }),
            )
        };
        let raw_descriptor = wgpu::RenderPipelineDescriptor {
            multiview: self.multiview,
            depth_stencil: self.depth_stencil.clone(),
            label: self.label.as_deref(),
            layout: layout.as_ref(),
            multisample: self.multisample,
            primitive: self.primitive,
            vertex: wgpu::VertexState {
                buffers: &vertex_buffer_layouts,
                entry_point: &self.vertex.entry_point,
                module,
            },
            fragment: self.fragment.as_ref().map(|state| wgpu::FragmentState {
                entry_point: &state.entry_point,
                module,
                targets: &state.targets,
            }),
        };
        device.create_render_pipeline(&raw_descriptor)
    }
}

impl Default for RenderPipelineDescriptor {
    fn default() -> Self {
        Self {
            label: Some("Pipeline".into()),
            layout: vec![],
            fragment: Some(FragmentState::default()),
            vertex: VertexState::default(),
            primitive: wgpu::PrimitiveState::default(),
            push_constant_ranges: vec![],
            depth_stencil: Some(wgpu::DepthStencilState {
                format: App::DEPTH_FORMAT,
                depth_write_enabled: true,
                depth_compare: wgpu::CompareFunction::GreaterEqual,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState {
                count: App::SAMPLE_COUNT,
                ..Default::default()
            },
            multiview: None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct VertexState {
    pub entry_point: Cow<'static, str>,
    pub buffers: Vec<VertexBufferLayout>,
}

impl Default for VertexState {
    fn default() -> Self {
        Self {
            entry_point: "vs_main".into(),
            buffers: vec![],
        }
    }
}

#[derive(Default, Clone, Debug, Hash, Eq, PartialEq)]
pub struct VertexBufferLayout {
    pub array_stride: BufferAddress,
    pub step_mode: VertexStepMode,
    pub attributes: Vec<VertexAttribute>,
}

impl VertexBufferLayout {
    pub fn from_vertex_formats<T: IntoIterator<Item = VertexFormat>>(
        step_mode: VertexStepMode,
        vertex_formats: T,
    ) -> Self {
        let mut offset = 0;
        let mut attributes = Vec::new();
        for (shader_location, format) in vertex_formats.into_iter().enumerate() {
            attributes.push(VertexAttribute {
                format,
                offset,
                shader_location: shader_location as u32,
            });
            offset += format.size();
        }

        VertexBufferLayout {
            array_stride: offset,
            step_mode,
            attributes,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct FragmentState {
    pub entry_point: Cow<'static, str>,
    pub targets: Vec<Option<ColorTargetState>>,
}

impl Default for FragmentState {
    fn default() -> Self {
        Self {
            entry_point: "fs_main".into(),
            targets: vec![Some(wgpu::TextureFormat::Bgra8UnormSrgb.into())],
        }
    }
}

/// Describes compute pipeline.
#[derive(Debug, Hash, PartialEq, Eq, Clone)]
pub struct ComputePipelineDescriptor {
    pub label: Option<Cow<'static, str>>,
    pub layout: Vec<bind_group_layout::BindGroupLayout>,
    pub push_constant_ranges: Vec<PushConstantRange>,
    pub entry_point: Cow<'static, str>,
}

impl ComputePipelineDescriptor {
    fn process(&self, device: &wgpu::Device, module: &wgpu::ShaderModule) -> wgpu::ComputePipeline {
        let bind_group_layouts = self.layout.iter().map(|x| x.value()).collect::<Vec<_>>();
        let layout = if self.push_constant_ranges.is_empty() && self.layout.is_empty() {
            None
        } else {
            Some(
                device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                    label: self.label.as_deref(),
                    push_constant_ranges: &self.push_constant_ranges,
                    bind_group_layouts: &bind_group_layouts,
                }),
            )
        };
        let raw_descriptor = wgpu::ComputePipelineDescriptor {
            label: self.label.as_deref(),
            layout: layout.as_ref(),
            module,
            entry_point: self.entry_point.as_ref(),
        };
        device.create_compute_pipeline(&raw_descriptor)
    }
}

impl Default for ComputePipelineDescriptor {
    fn default() -> Self {
        Self {
            label: Some("Compute Pipeline".into()),
            layout: vec![],
            push_constant_ranges: vec![],
            entry_point: "cs_main".into(),
        }
    }
}
