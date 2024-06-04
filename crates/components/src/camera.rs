use dolly::{
    prelude::{Position, Smooth, YawPitch},
    rig::CameraRig,
};
use glam::{vec4, Mat4, Quat, Vec2, Vec3, Vec4};
use wgpu::util::DeviceExt;

use crate::{
    bind_group_layout::{self, WrappedBindGroupLayout},
    NonZeroSized,
};

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct CameraUniform {
    pub view_position: [f32; 4],
    pub projection: Mat4,
    pub view: Mat4,
    pub clip_to_world: Mat4,
    pub prev_world_to_clip: Mat4,
    frustum: [f32; 4],
    zfar: f32,
    znear: f32,
    pub jitter: [f32; 2],
    prev_jitter: [f32; 2],
    _padding: [f32; 2],
}

impl Default for CameraUniform {
    fn default() -> Self {
        Self {
            view_position: [0.0; 4],
            projection: Mat4::IDENTITY,
            view: Mat4::IDENTITY,
            clip_to_world: Mat4::IDENTITY,
            prev_world_to_clip: Mat4::IDENTITY,
            frustum: [0.; 4],
            zfar: f32::INFINITY,
            znear: Camera::ZNEAR,
            jitter: [0.; 2],
            prev_jitter: [0.; 2],
            _padding: [0.; 2],
        }
    }
}

pub struct CameraUniformBinding {
    buffer: wgpu::Buffer,
    pub binding: wgpu::BindGroup,
    pub bind_group_layout: bind_group_layout::BindGroupLayout,
}

impl CameraUniformBinding {
    pub const DESC: wgpu::BindGroupLayoutDescriptor<'static> = wgpu::BindGroupLayoutDescriptor {
        label: Some("Camera Bind Group Layout"),
        entries: &[wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::VERTEX_FRAGMENT.union(wgpu::ShaderStages::COMPUTE),
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: Some(CameraUniform::NSIZE),
            },
            count: None,
        }],
    };

    pub fn new(device: &wgpu::Device) -> Self {
        let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Camera Buffer"),
            contents: bytemuck::bytes_of(&CameraUniform::default()),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let bind_group_layout = device.create_bind_group_layout_wrap(&Self::DESC);
        let binding = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Camera Bind Group"),
            layout: &bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: buffer.as_entire_binding(),
            }],
        });

        Self {
            buffer,
            binding,
            bind_group_layout,
        }
    }

    pub fn update(&mut self, queue: &wgpu::Queue, camera_uniform: &CameraUniform) {
        queue.write_buffer(&self.buffer, 0, bytemuck::bytes_of(camera_uniform));
    }

    pub fn buffer(&self) -> &wgpu::Buffer {
        &self.buffer
    }
}

#[derive(Debug)]
pub struct Camera {
    pub rig: CameraRig,
    pub position: Vec3,
    pub rotation: Quat,
    pub aspect: f32,
    pub jitter: Vec2,
}

impl Camera {
    pub const ZNEAR: f32 = 0.001;
    pub const FOVY: f32 = std::f32::consts::PI / 2.0;

    pub fn new(position: Vec3, yaw: f32, pitch: f32) -> Self {
        let rig: CameraRig = CameraRig::builder()
            .with(Position::new(position))
            .with(YawPitch::new().yaw_degrees(yaw).pitch_degrees(pitch))
            .with(Smooth::new_position_rotation(1.0, 1.5))
            .build();
        Self {
            rig,
            aspect: 1.25,
            position,
            rotation: Quat::IDENTITY,
            jitter: Vec2::ZERO,
        }
    }

    pub fn build_projection_view_matrix(&self) -> (Mat4, Mat4) {
        let tr = self.rig.final_transform;
        let position = tr.position.into();
        let forward: Vec3 = tr.forward();
        let view = Mat4::look_at_rh(position, position + forward, tr.up());
        let proj = Mat4::perspective_infinite_reverse_rh(Self::FOVY, self.aspect, Self::ZNEAR);
        (proj, view)
    }

    pub fn get_uniform(&self, previous: Option<&CameraUniform>) -> CameraUniform {
        let position: Vec3 = self.rig.final_transform.position.into();
        let pos = Vec4::from((position, 1.));
        let (mut projection, view) = self.build_projection_view_matrix();
        projection.z_axis[0] += self.jitter.x;
        projection.z_axis[1] += self.jitter.y;
        let proj_view = projection * view;

        // https://github.com/zeux/niagara/blob/3fafe000ba8fe6e309b41e915b81242b4ca3db28/src/niagara.cpp#L836-L852
        let perspective_t = projection.transpose();
        // x + w < 0
        let frustum_x = (perspective_t.col(3) + perspective_t.col(0)).normalize();
        // y + w < 0
        let frustum_y = (perspective_t.col(3) + perspective_t.col(1)).normalize();
        let frustum = vec4(frustum_x.x, frustum_x.z, frustum_y.y, frustum_y.z);

        let (prev_world_to_clip, prev_jitter) = if let Some(prev) = previous {
            ((prev.projection * prev.view), prev.jitter)
        } else {
            (proj_view, [0.; 2])
        };

        CameraUniform {
            view_position: pos.to_array(),
            projection,
            view,
            clip_to_world: proj_view.inverse(),
            prev_world_to_clip,
            frustum: frustum.to_array(),
            zfar: f32::INFINITY,
            znear: Camera::ZNEAR,
            jitter: self.jitter.to_array(),
            prev_jitter,
            _padding: [0.; 2],
        }
    }

    pub fn aspect(&self) -> f32 {
        self.aspect
    }
}
