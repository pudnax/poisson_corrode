#![allow(clippy::new_without_default)]

use components::FpsCounter;
use eyre::Result;
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

use glam::vec3;
use log::warn;
use wgpu::SurfaceError;
use winit::{
    dpi::PhysicalSize,
    event::{KeyEvent, WindowEvent},
    event_loop::ControlFlow,
    keyboard::{KeyCode, NamedKey},
    window::WindowAttributes,
};

pub use crate::app::App;
mod app;
pub mod models;
pub mod pass;
pub mod prelude;

pub use crate::models::GltfDocument;
pub use app::DEFAULT_SAMPLER_DESC;
pub use app::{
    gbuffer::GBuffer,
    global_ubo::{GlobalUniformBinding, GlobalsBindGroup, Uniform},
    pipeline,
    state::AppState,
    ProfilerCommandEncoder, RenderContext, UpdateContext, ViewTarget,
};
pub use components::{
    bind_group_layout::{self, WrappedBindGroupLayout},
    shared::*,
    Camera, Gpu, LerpExt, NonZeroSized, ResizableBuffer, ResizableBufferExt, Watcher,
    {CameraUniform, CameraUniformBinding}, {KeyMap, KeyboardMap},
};
// pub use egui;
pub use pools::*;
pub use winit::dpi::LogicalSize;

pub const UPDATES_PER_SECOND: u32 = 60;
pub const FIXED_TIME_STEP: f64 = 1. / UPDATES_PER_SECOND as f64;
pub const MAX_FRAME_TIME: f64 = 15. * FIXED_TIME_STEP; // 0.25;

pub const SHADER_FOLDER: &str = "shaders";

pub trait Example: 'static + Sized {
    fn name() -> &'static str {
        "Example"
    }

    fn init(gpu: &mut App) -> Result<Self>;
    fn setup_scene(&mut self, _app: &mut App) -> Result<()> {
        Ok(())
    }
    fn update(&mut self, _ctx: UpdateContext) {}
    fn resize(&mut self, _gpu: &Gpu, _width: u32, _height: u32) {}
    fn render(&mut self, ctx: RenderContext);
}

// pub fn run_default<E: Example>() -> eyre::Result<()> {
//     let window = winit::window::WindowBuilder::new()
//         .with_title(E::name())
//         .with_inner_size(LogicalSize::new(1280, 1024));
//
//     let camera = Camera::new(vec3(0., 0., 0.), 0., 0.);
//     run::<E>(window, camera)
// }
//
// pub fn run<E: Example>(window_builder: WindowBuilder, mut camera: Camera) -> eyre::Result<()> {
//     eyre::install()?;
//     env_logger::builder()
//         .parse_env(env_logger::Env::default().filter_or(env_logger::DEFAULT_FILTER_ENV, "info"))
//         .filter_module("wgpu_core", log::LevelFilter::Warn)
//         .filter_module("wgpu_hal", log::LevelFilter::Warn)
//         .filter_module("MANGOHUD", log::LevelFilter::Warn)
//         .filter_module("winit", log::LevelFilter::Warn)
//         .filter_module("naga", log::LevelFilter::Error)
//         .init();
//
//     let event_loop = winit::event_loop::EventLoopBuilder::with_user_event().build()?;
//     let window = window_builder.with_title(E::name()).build(&event_loop)?;
//
//     let PhysicalSize { width, height } = window.inner_size();
//     camera.aspect = width as f32 / height as f32;
//
//     let keyboard_map = {
//         use KeyCode::*;
//         KeyboardMap::new()
//             .bind(KeyW, KeyMap::new("move_fwd", 1.0))
//             .bind(KeyS, KeyMap::new("move_fwd", -1.0))
//             .bind(KeyD, KeyMap::new("move_right", 1.0))
//             .bind(KeyA, KeyMap::new("move_right", -1.0))
//             .bind(KeyQ, KeyMap::new("move_up", 1.0))
//             .bind(KeyE, KeyMap::new("move_up", -1.0))
//             .bind(ShiftLeft, KeyMap::new("boost", 1.0))
//             .bind(ControlLeft, KeyMap::new("boost", -1.0))
//     };
//     let mut app_state = AppState::new(camera, Some(keyboard_map));
//
//     let watcher = Watcher::new(event_loop.create_proxy())?;
//
//     let mut app = App::new(&window, watcher)?;
//     let info = app.get_info();
//     println!("{info}");
//
//     let mut example = E::init(&mut app)?;
//
//     let now = std::time::Instant::now();
//     app.setup_scene(&mut example)?;
//     println!("Scene finished: {:?}", now.elapsed());
//
//     let mut current_instant = Instant::now();
//     let mut accumulated_time = 0.;
//     let mut fps_counter = FpsCounter::new();
//
//     event_loop.run(move |event, elwt| {
//         *control_flow = ControlFlow::Wait;
//
//         match event {
//             Event::MainEventsCleared => {
//                 let new_instant = Instant::now();
//                 let frame_time = new_instant
//                     .duration_since(current_instant)
//                     .as_secs_f64()
//                     .min(MAX_FRAME_TIME);
//                 current_instant = new_instant;
//
//                 let mut actions = vec![];
//                 accumulated_time += frame_time;
//                 while accumulated_time >= FIXED_TIME_STEP {
//                     app_state.input.tick();
//                     actions.extend(app_state.update(FIXED_TIME_STEP));
//
//                     accumulated_time -= FIXED_TIME_STEP;
//                 }
//                 app.update(&mut app_state, actions, |ctx| example.update(ctx))
//                     .unwrap();
//                 app_state.input.mouse_state.refresh();
//             }
//             Event::RedrawEventsCleared => window.request_redraw(),
//             Event::RedrawRequested(_) => {
//                 app_state.dt = fps_counter.record();
//                 if let Err(err) = app.render(&window, &app_state, |ctx| example.render(ctx)) {
//                     eprintln!("get_current_texture error: {:?}", err);
//                     match err {
//                         SurfaceError::Lost | SurfaceError::Outdated => {
//                             warn!("render: Outdated Surface");
//                             app.surface.configure(app.device(), &app.surface_config);
//                             window.request_redraw();
//                         }
//                         SurfaceError::OutOfMemory => elwt.control_flow(),
//                         SurfaceError::Timeout => warn!("Surface Timeout"),
//                     }
//                 }
//             }
//             Event::WindowEvent {
//                 event: WindowEvent::Resized(PhysicalSize { width, height }), // | WindowEvent::ScaleFactorChanged {
//                                                                              //     new_inner_size: &mut PhysicalSize { width, height },
//                                                                              //     ..
//                                                                              // }
//                 ..
//             } => {
//                 if width != 0 && height != 0 {
//                     app_state.camera.aspect = width as f32 / height as f32;
//                     example.resize(&app.gpu, width, height);
//                     app.resize(width, height);
//                 }
//             }
//             Event::WindowEvent {
//                 event:
//                     WindowEvent::CloseRequested
//                     | WindowEvent::KeyboardInput {
//                         event:
//                             KeyEvent {
//                                 logical_key: Key::Named(NamedKey::Escape),
//                                 state: ElementState::Pressed,
//                                 ..
//                             },
//                         ..
//                     },
//                 ..
//             } => *control_flow = ControlFlow::Exit,
//             Event::DeviceEvent { event, .. } => app_state.input.on_device_event(&event),
//             Event::WindowEvent { event, .. } => {
//                 if app.egui_state.on_event(&app.egui_context, &event).consumed {
//                     return;
//                 }
//
//                 app_state.input.on_window_event(&window, &event);
//             }
//             Event::UserEvent(path) => {
//                 app.handle_events(path);
//             }
//             Event::LoopDestroyed => {
//                 println!("// End from the loop. Bye bye~⏎ ");
//             }
//             _ => {}
//         }
//     });
//     Ok(())
// }

pub fn run_default<E: Example>() -> eyre::Result<()> {
    let camera = Camera::new(vec3(0., 0., 0.), 0., 0.);
    run::<E>(WindowAttributes::default(), camera)
}

pub fn run<E: Example>(
    mut window_attributes: WindowAttributes,
    camera: Camera,
) -> eyre::Result<()> {
    env_logger::builder()
        .parse_env(env_logger::Env::default().filter_or(env_logger::DEFAULT_FILTER_ENV, "info"))
        .filter_module("wgpu_core", log::LevelFilter::Warn)
        .filter_module("wgpu_hal", log::LevelFilter::Warn)
        .filter_module("MANGOHUD", log::LevelFilter::Warn)
        .filter_module("winit", log::LevelFilter::Warn)
        .filter_module("naga", log::LevelFilter::Error)
        .init();

    let event_loop = winit::event_loop::EventLoop::<PathBuf>::with_user_event().build()?;
    event_loop.set_control_flow(ControlFlow::Wait);

    window_attributes = window_attributes.with_title(E::name());

    let watcher = Watcher::new(event_loop.create_proxy())?;

    let keyboard_map = {
        use KeyCode::*;
        KeyboardMap::new()
            .bind(KeyW, KeyMap::new("move_fwd", 1.0))
            .bind(KeyS, KeyMap::new("move_fwd", -1.0))
            .bind(KeyD, KeyMap::new("move_right", 1.0))
            .bind(KeyA, KeyMap::new("move_right", -1.0))
            .bind(KeyQ, KeyMap::new("move_up", 1.0))
            .bind(KeyE, KeyMap::new("move_up", -1.0))
            .bind(ShiftLeft, KeyMap::new("boost", 1.0))
            .bind(ControlLeft, KeyMap::new("boost", -1.0))
    };
    let app_state = AppState::new(camera, Some(keyboard_map));

    let current_instant = Instant::now();
    let accumulated_time = 0.;
    let fps_counter = FpsCounter::new();

    let mut app_runner = AppRunner::<E> {
        window_attributes,
        current_instant,
        accumulated_time,
        fps_counter,
        file_watcher: Some(watcher),
        window: None,
        renderer: None,
        state: app_state,
    };

    event_loop.run_app(&mut app_runner)?;

    Ok(())
}

struct AppRunner<'a, E: Example> {
    window_attributes: WindowAttributes,
    current_instant: Instant,
    accumulated_time: f64,
    fps_counter: FpsCounter,
    file_watcher: Option<Watcher>,
    window: Option<(wgpu::Surface<'a>, winit::window::Window)>,
    state: AppState,
    renderer: Option<(App, E)>,
}

use winit::application::ApplicationHandler;
impl<'a, E: Example> ApplicationHandler<PathBuf> for AppRunner<'a, E> {
    fn resumed(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::VULKAN,
            dx12_shader_compiler: wgpu::Dx12Compiler::Fxc,
            flags: wgpu::InstanceFlags::default(),
            gles_minor_version: wgpu::Gles3MinorVersion::Automatic,
        });

        let window = event_loop
            .create_window(self.window_attributes.clone())
            .expect("Failed to create window");
        let surface = unsafe {
            let surface_target = wgpu::SurfaceTargetUnsafe::from_window(&window)
                .expect("Failed to create surface target");
            instance
                .create_surface_unsafe(surface_target)
                .expect("Failed to create surface")
        };

        let mut app = App::new(
            instance,
            &window,
            &surface,
            self.file_watcher
                .take()
                .expect("File watcher hasn't been initialized'"),
        )
        .expect("Failed to create render state");
        let info = app.get_info();
        println!("{info}");

        let mut example = E::init(&mut app).expect("Failed to create Example app");

        let now = std::time::Instant::now();
        app.setup_scene(&mut example)
            .expect("Failed to setup the scene");
        println!("Scene finished: {:?}", now.elapsed());

        self.renderer = Some((app, example));
        self.window = Some((surface, window));
    }

    fn new_events(
        &mut self,
        event_loop: &winit::event_loop::ActiveEventLoop,
        _cause: winit::event::StartCause,
    ) {
        dbg!(_cause);
        let new_instant = Instant::now();
        let frame_time = new_instant
            .duration_since(self.current_instant)
            .as_secs_f64()
            .min(MAX_FRAME_TIME);
        self.current_instant = new_instant;

        let mut actions = vec![];
        self.accumulated_time += frame_time;
        if let Some((_, window)) = self.window.as_ref() {
            if self.accumulated_time >= FIXED_TIME_STEP {
                event_loop.set_control_flow(ControlFlow::WaitUntil(
                    new_instant + Duration::from_secs_f64(FIXED_TIME_STEP),
                ));
                window.request_redraw();
            }
        }
        while self.accumulated_time >= FIXED_TIME_STEP {
            self.state.input.tick();
            actions.extend(self.state.update(FIXED_TIME_STEP));

            self.accumulated_time -= FIXED_TIME_STEP;
        }
        if let Some((renderer, example)) = self.renderer.as_mut() {
            renderer
                .update(&mut self.state, actions, |ctx| example.update(ctx))
                .unwrap();
        }
        self.state.input.mouse_state.refresh();
    }

    fn window_event(
        &mut self,
        event_loop: &winit::event_loop::ActiveEventLoop,
        _window_id: winit::window::WindowId,
        event: WindowEvent,
    ) {
        let Some((surface, window)) = self.window.as_ref() else {
            return;
        };
        let Some((render, example)) = self.renderer.as_mut() else {
            return;
        };

        self.state.input.on_window_event(window, &event);

        match event {
            WindowEvent::CloseRequested
            | WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        logical_key: winit::keyboard::Key::Named(NamedKey::Escape),
                        ..
                    },
                ..
            } => event_loop.exit(),
            WindowEvent::Resized(PhysicalSize { width, height }) => {
                if width != 0 && height != 0 {
                    self.state.camera.aspect = width as f32 / height as f32;
                    surface.configure(render.device(), &render.surface_config);
                    example.resize(&render.gpu, width, height);
                    render.resize(width, height);
                }
            }
            WindowEvent::RedrawRequested => {
                self.state.dt = self.fps_counter.record();
                if let Err(err) =
                    render.render(window, surface, &self.state, |ctx| example.render(ctx))
                {
                    eprintln!("get_current_texture error: {:?}", err);
                    match err {
                        SurfaceError::Lost | SurfaceError::Outdated => {
                            warn!("render: Outdated Surface");
                            surface.configure(render.device(), &render.surface_config);
                            window.request_redraw();
                        }
                        SurfaceError::OutOfMemory => warn!("Low Memory!"),
                        SurfaceError::Timeout => warn!("Surface Timeout"),
                    }
                }
                // window.request_redraw();
            }
            _ => {}
        }
    }

    fn user_event(&mut self, _event_loop: &winit::event_loop::ActiveEventLoop, event: PathBuf) {
        let Some((render, _example)) = self.renderer.as_mut() else {
            return;
        };
        render.handle_events(event);
    }

    fn device_event(
        &mut self,
        _event_loop: &winit::event_loop::ActiveEventLoop,
        _device_id: winit::event::DeviceId,
        event: winit::event::DeviceEvent,
    ) {
        self.state.input.on_device_event(&event);
    }

    fn exiting(&mut self, _event_loop: &winit::event_loop::ActiveEventLoop) {
        println!("// End from the loop. Bye bye~⏎ ");
    }
}
