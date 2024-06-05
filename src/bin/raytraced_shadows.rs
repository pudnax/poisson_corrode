
use eyre::Result;
use voidin::*;
use winit::window::WindowAttributes;

struct Shadows {
    visibility_pass: pass::visibility::Visibility,
    shading_pass: pass::shading::ShadingPass,
}

impl Example for Shadows {
    fn name() -> &'static str {
        "Raytraced Shadows"
    }

    fn init(app: &mut App) -> Result<Self> {
        let visibility_pass = pass::visibility::Visibility::new(&app.world)?;
        let shading_pass = pass::shading::ShadingPass::new(
            "src/bin/raytraced_shadows.wgsl",
            &app.world,
            &app.gbuffer,
        )?;

        Ok(Self {
            visibility_pass,
            shading_pass,
        })
    }

    fn setup_scene(&mut self, app: &mut App) -> Result<()> {
        app.world
            .get_mut::<LightPool>()?
            .add_point_light(&[Light::new(vec3(-3., 8.5, 10.), 100., vec3(1., 1., 1.))]);
        let mut instances = vec![];

        instances.push(Instance::new(
            Mat4::from_scale(Vec3::splat(200.)),
            MeshPool::HORIZONTAL_PLANE_MESH,
            MaterialId::default(),
        ));

        let (width, height) = (5., 10.);
        let box_mesh = models::ObjModel::import(app, "assets/cube/cube.obj")?;
        let n = 11;
        for i in (0..n).map(|i| ((i as f32) / (n as f32)) * 2. - 1.) {
            for (mesh, material) in &box_mesh {
                instances.push(Instance::new(
                    Mat4::from_translation(vec3(20. * i * width, height / 2., 0.))
                        * Mat4::from_scale(vec3(width, height, width) / 2.),
                    *mesh,
                    *material,
                ));
            }
        }

        instances.push(Instance::new(
            Mat4::IDENTITY,
            MeshPool::SPHERE_1_MESH,
            MaterialId::default(),
        ));

        let gltf_ferris = GltfDocument::import(app, "assets/ferris3d_v1.0.glb")?;
        instances.extend(gltf_ferris.get_scene_instances(
            Mat4::from_translation(vec3(-3., 1.0, -4.)) * Mat4::from_scale(Vec3::splat(3.)),
        ));

        app.get_instance_pool_mut().add(&instances);

        Ok(())
    }

    fn update(&mut self, _ctx: UpdateContext) {}

    fn resize(&mut self, _gpu: &Gpu, _width: u32, _height: u32) {}

    fn render(
        &mut self,
        mut ctx @ RenderContext {
            world,
            gbuffer,
            view_target,
            draw_cmd_bind_group,
            draw_cmd_buffer,
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
    let window = WindowAttributes::default();

    let camera = Camera::new(vec3(0., 6., 0.), 0., 0.);
    run::<Shadows>(window, camera)
}
