use dolly::prelude::{Position, YawPitch};
use glam::Vec3;

use components::{
    Camera, {Input, KeyboardMap, KeyboardState},
};
use winit::keyboard::KeyCode;

pub enum StateAction {
    Screenshot,
    StartRecording,
    FinishRecording,
}

pub struct AppState {
    pub frame_count: u64,
    pub total_time: f64,
    pub camera: Camera,
    pub input: Input,
    pub keyboard_map: KeyboardMap,
    pub dt: f64,
    recording: bool,
}

impl AppState {
    pub fn new(camera: Camera, keyboard_map: Option<KeyboardMap>) -> Self {
        Self {
            input: Input::new(),
            frame_count: 0,
            total_time: 0.,
            camera,
            keyboard_map: keyboard_map.unwrap_or_default(),
            recording: false,
            dt: 0.,
        }
    }

    pub fn update(&mut self, dt: f64) -> Vec<StateAction> {
        let mut actions = vec![];

        self.total_time += dt;
        self.frame_count = self.frame_count.wrapping_add(1);

        if self.input.mouse_state.left_held() {
            let sensitivity = 0.5;
            self.camera.rig.driver_mut::<YawPitch>().rotate_yaw_pitch(
                -sensitivity * self.input.mouse_state.delta.x,
                -sensitivity * self.input.mouse_state.delta.y,
            );
        }

        let moves = self.keyboard_map.map(&self.input.keyboard_state);
        let rotation = self.camera.rig.final_transform.rotation.into();
        let position = self.camera.rig.final_transform.position.into();
        let move_vec = rotation
            * Vec3::new(moves["move_right"], moves["move_up"], -moves["move_fwd"])
                .clamp_length_max(1.0)
            * 4.0f32.powf(moves["boost"]);

        self.camera
            .rig
            .driver_mut::<Position>()
            .translate(move_vec * dt as f32 * 5.0);

        self.camera.rig.update(dt as _);

        self.camera.position = position;
        self.camera.rotation = rotation;

        if self.keyboard().was_just_pressed(KeyCode::F3) {
            actions.push(StateAction::Screenshot);
        };
        if self.keyboard().was_just_pressed(KeyCode::F4) {
            if !self.recording {
                actions.push(StateAction::StartRecording)
            } else {
                actions.push(StateAction::FinishRecording)
            }
            self.recording = !self.recording;
        };
        actions
    }

    fn keyboard(&self) -> &KeyboardState {
        &self.input.keyboard_state
    }
}
