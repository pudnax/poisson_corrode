mod cube;
mod gltf_model;
mod plane;
mod sphere;

use color_eyre::{
    eyre::{eyre, Context},
    Result,
};
use glam::{Vec3, Vec4};
use std::path::Path;

pub use cube::cube_mesh;
pub use gltf_model::*;
pub use plane::plane_mesh;
pub use sphere::make_uv_sphere;

use crate::{
    app::{
        material::{Material, MaterialId},
        mesh::{MeshId, MeshRef},
        App,
    },
    utils::mesh_bounding_sphere,
};

pub struct ObjModel;

impl ObjModel {
    pub fn import(app: &mut App, path: impl AsRef<Path>) -> Result<Vec<(MeshId, MaterialId)>> {
        let name = path.as_ref().file_name();
        log::info!("Started processing model: {name:?}",);
        let (model_meshes, model_materials) =
            tobj::load_obj(path.as_ref(), &tobj::GPU_LOAD_OPTIONS)
                .with_context(|| eyre!("Failed to open file: {}", path.as_ref().display()))?;

        let mut materials = vec![];
        if let Ok(model_materials) = model_materials {
            for material in model_materials {
                let base_color = Vec3::from_array(material.diffuse.unwrap_or([1., 1., 1.]));
                let material_id = app.get_material_pool_mut().add(Material {
                    base_color: base_color.extend(0.5),
                    ..Default::default()
                });
                materials.push(material_id);
            }
        }

        let mut meshes = vec![];
        for mesh in model_meshes.iter().map(|m| &m.mesh) {
            let mesh_id = app.add_mesh(MeshRef {
                vertices: bytemuck::cast_slice(&mesh.positions),
                normals: bytemuck::cast_slice(&mesh.normals),
                tangents: &vec![Vec4::ZERO; mesh.positions.len()],
                tex_coords: bytemuck::cast_slice(&mesh.texcoords),
                indices: bytemuck::cast_slice(&mesh.indices),
                bounding_sphere: mesh_bounding_sphere(bytemuck::cast_slice(&mesh.positions)),
            });
            let material_id = match mesh.material_id {
                Some(id) => materials[id],
                None => MaterialId::default(),
            };
            meshes.push((mesh_id, material_id));
        }

        app.get_texture_pool_mut().update_bind_group();
        Ok(meshes)
    }
}
