
use app::make_uv_sphere;
use eyre::Result;
use rand::Rng;
use voidin::*;
use winit::window::WindowAttributes;

struct Model {
    visibility_pass: pass::visibility::Visibility,

    shading_pass: pass::shading::ShadingPass,

    postprocess_pass: pass::postprocess::PostProcess,

    update_pass: pass::compute_update::ComputeUpdate,

    taa_pass: pass::taa::Taa,

    moving_instances: ResizableBuffer<InstanceId>,
    moving_instances_bind_group: wgpu::BindGroup,
}

impl Example for Model {
    fn name() -> &'static str {
        "Model"
    }

    fn init(app: &mut App) -> Result<Self> {
        let visibility_pass = pass::visibility::Visibility::new(&app.world)?;

        let shading_pass =
            pass::shading::ShadingPass::new("shaders/shading.wgsl", &app.world, &app.gbuffer)?;

        let postprocess_pass =
            pass::postprocess::PostProcess::new(&app.world, "shaders/postprocess.wgsl")?;

        let update_pass =
            pass::compute_update::ComputeUpdate::new(&app.world, "shaders/compute_update.wgsl")?;

        let taa_pass = pass::taa::Taa::new(
            &app.world,
            &app.gbuffer,
            app.surface_config.width,
            app.surface_config.height,
        )?;
        let moving_instances = app
            .device()
            .create_resizable_buffer(wgpu::BufferUsages::STORAGE);
        let moving_instances_bind_group =
            moving_instances.create_storage_read_bind_group(&mut app.world);

        Ok(Self {
            visibility_pass,
            shading_pass,
            postprocess_pass,
            update_pass,
            taa_pass,

            moving_instances,
            moving_instances_bind_group,
        })
    }

    fn setup_scene(&mut self, app: &mut App) -> Result<()> {
        use std::f32::consts::PI;
        let mut instances = vec![];

        app.world
            .get_mut::<LightPool>()?
            .add_point_light(&[Light::new(vec3(0., 0.5, 0.), 10., vec3(1., 1., 1.))]);

        app.add_area_light(
            vec3(1., 1., 1.),
            7.,
            (5., 8.).into(),
            Mat4::from_translation(vec3(0., 10., 15.)) * Mat4::from_rotation_x(-PI / 4.),
        )?;
        app.add_area_light(
            vec3(1., 1., 1.),
            7.,
            (5., 8.).into(),
            Mat4::from_translation(vec3(0., 10., -25.)) * Mat4::from_rotation_x(-3. * PI / 4.),
        )?;

        let gltf_scene = GltfDocument::import(
            app,
            "assets/glTF-Sample-Models/2.0/Sponza/glTF/Sponza.gltf",
            // "assets/glTF-Sample-Models/2.0/AntiqueCamera/glTF/AntiqueCamera.gltf",
            // "assets/glTF-Sample-Models/2.0/Buggy/glTF-Binary/Buggy.glb",
            // "assets/glTF-Sample-Models/2.0/FlightHelmet/glTF/FlightHelmet.gltf",
            // "assets/glTF-Sample-Models/2.0/DamagedHelmet/glTF-Binary/DamagedHelmet.glb",
        )?;

        instances.extend(gltf_scene.get_scene_instances(
            Mat4::from_rotation_y(PI / 2.)
                * Mat4::from_translation(vec3(7., -5., 1.))
                * Mat4::from_scale(Vec3::splat(3.)),
        ));

        let helmet = GltfDocument::import(
            app,
            "assets/glTF-Sample-Models/2.0/DamagedHelmet/glTF-Binary/DamagedHelmet.glb",
        )?;
        instances.extend(helmet.get_scene_instances(
            Mat4::from_translation(vec3(0., 0., 9.)) * Mat4::from_scale(Vec3::splat(3.)),
        ));

        let gltf_ferris = GltfDocument::import(app, "assets/ferris3d_v1.0.glb")?;
        instances.extend(gltf_ferris.get_scene_instances(
            Mat4::from_translation(vec3(-3., -5.0, -4.)) * Mat4::from_scale(Vec3::splat(3.)),
        ));
        instances.extend(gltf_ferris.get_scene_instances(
            Mat4::from_translation(vec3(2., -5.0, -2.)) * Mat4::from_scale(Vec3::splat(3.)),
        ));
        gltf_ferris.get_scene_instances(
            Mat4::from_translation(vec3(2., -5.0, -2.)) * Mat4::from_scale(Vec3::splat(3.)),
        );
        app.world.get_mut::<InstancePool>()?.add(&instances);

        let sphere_mesh = make_uv_sphere(1.0, 10);
        let sphere_mesh_id = app.get_mesh_pool_mut().add(sphere_mesh.as_ref());

        let mut moving_instances = vec![];
        let mut rng = rand::thread_rng();
        let num = 10;
        for i in 0..num {
            let r = 3.5;
            let angle = 2. * PI * (i as f32) / num as f32;
            let x = r * angle.cos();
            let y = r * angle.sin();

            moving_instances.push(Instance::new(
                Mat4::from_translation(vec3(x, y, -17.)),
                sphere_mesh_id,
                MaterialId::new(rng.gen_range(0..app.get_material_pool().num_materials() as u32)),
            ));

            moving_instances.extend(gltf_ferris.get_scene_instances(
                Mat4::from_translation(vec3(x, y + 0., -9.))
                    * Mat4::from_rotation_z(angle)
                    * Mat4::from_scale(Vec3::splat(2.5)),
            ));
        }

        let moving_instances_id = app.world.get_mut::<InstancePool>()?.add(&moving_instances);
        self.moving_instances.push(&app.gpu, &moving_instances_id);
        self.moving_instances_bind_group = self
            .moving_instances
            .create_storage_read_bind_group(&mut app.world);

        Ok(())
    }

    fn update(&mut self, mut ctx: UpdateContext) {
        ctx.app_state.camera.jitter =
            self.taa_pass
                .get_jitter(ctx.app_state.frame_count as u32, ctx.width, ctx.height);

        let resources = pass::compute_update::ComputeUpdateResource {
            idx_bind_group: &self.moving_instances_bind_group,
            dispatch_size: self.moving_instances.len() as u32,
        };
        self.update_pass
            .record(ctx.world, &mut ctx.encoder, resources);
    }

    fn resize(&mut self, gpu: &Gpu, width: u32, height: u32) {
        self.taa_pass.resize(gpu.device(), width, height);
    }

    fn render(
        &mut self,
        mut ctx @ RenderContext {
            world,
            gbuffer,
            view_target,
            draw_cmd_bind_group,
            draw_cmd_buffer,
            width,
            height,
            ..
        }: RenderContext,
    ) {
        let encoder = &mut ctx.encoder;

        self.visibility_pass.record(
            world,
            encoder,
            pass::visibility::VisibilityResource {
                gbuffer,
                draw_cmd_buffer,
                draw_cmd_bind_group,
            },
        );

        self.shading_pass.record(
            world,
            encoder,
            pass::shading::ShadingResource {
                gbuffer,
                view_target,
            },
        );

        self.taa_pass.record(
            world,
            encoder,
            pass::taa::TaaResource {
                gbuffer,
                view_target,
                width_height: (width, height),
            },
        );

        self.postprocess_pass.record(
            world,
            encoder,
            pass::postprocess::PostProcessResource { view_target },
        );

        // ctx.ui(|egui_ctx| {
        //     egui::Window::new("debug").show(egui_ctx, |ui| {
        //         ui.label(format!(
        //             "Fps: {:.04?}",
        //             Duration::from_secs_f64(ctx.app_state.dt)
        //         ));
        //     });
        // });
    }
}

fn main() -> Result<()> {
    let window = WindowAttributes::default().with_inner_size(LogicalSize::new(1280, 1024));

    let camera = Camera::new(vec3(2., 5., 12.), 0., -20.);
    run::<Model>(window, camera)
}
