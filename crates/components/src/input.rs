use ahash::AHashMap;
use glam::{vec2, Vec2};
use winit::{
    dpi::{PhysicalPosition, PhysicalSize},
    event::{DeviceEvent, ElementState, KeyEvent, MouseButton, MouseScrollDelta, WindowEvent},
    keyboard::{KeyCode, PhysicalKey},
    window::Window,
};

#[derive(Clone, Debug, Default)]
pub struct KeyState {
    pub ticks: u32,
}

#[derive(Default, Clone, Debug)]
pub struct KeyboardState {
    keys_down: AHashMap<KeyCode, KeyState>,
}

impl KeyboardState {
    pub fn is_down(&self, key: KeyCode) -> bool {
        self.get_down(key).is_some()
    }

    pub fn was_just_pressed(&self, key: KeyCode) -> bool {
        self.get_down(key).map(|s| s.ticks == 1).unwrap_or_default()
    }

    pub fn get_down(&self, key: KeyCode) -> Option<&KeyState> {
        self.keys_down.get(&key)
    }
}

#[derive(Clone, Copy, Debug)]
pub struct MouseState {
    pub screen_position: Vec2,
    pub delta: Vec2,
    pub scroll: f32,
    pub buttons_held: u32,
    pub buttons_pressed: u32,
    pub buttons_released: u32,
}

impl MouseState {
    const LEFT: u32 = 0;
    const MIDDLE: u32 = 1;
    const RIGHT: u32 = 2;

    pub fn refresh(&mut self) {
        self.delta = vec2(0., 0.);
        self.scroll = 0.;
        self.buttons_pressed = 0;
        self.buttons_released = 0;
    }

    pub fn left_pressed(&self) -> bool {
        self.buttons_pressed & (1 << Self::LEFT) != 0
    }
    pub fn middle_pressed(&self) -> bool {
        self.buttons_pressed & (1 << Self::MIDDLE) != 0
    }
    pub fn right_pressed(&self) -> bool {
        self.buttons_pressed & (1 << Self::RIGHT) != 0
    }
    pub fn left_released(&self) -> bool {
        self.buttons_released & (1 << Self::LEFT) != 0
    }
    pub fn middle_released(&self) -> bool {
        self.buttons_released & (1 << Self::MIDDLE) != 0
    }
    pub fn right_released(&self) -> bool {
        self.buttons_released & (1 << Self::RIGHT) != 0
    }
    pub fn left_held(&self) -> bool {
        self.buttons_held & (1 << Self::LEFT) != 0
    }
    pub fn middle_held(&self) -> bool {
        self.buttons_held & (1 << Self::MIDDLE) != 0
    }
    pub fn right_held(&self) -> bool {
        self.buttons_held & (1 << Self::RIGHT) != 0
    }
}

impl Default for MouseState {
    fn default() -> Self {
        Self {
            screen_position: Vec2::ZERO,
            delta: Vec2::ZERO,
            scroll: 0.,
            buttons_held: 0,
            buttons_pressed: 0,
            buttons_released: 0,
        }
    }
}

pub type Action = &'static str;

pub struct KeyMap {
    action: Action,
    multiplier: f32,
}

impl KeyMap {
    pub fn new(action: Action, multiplier: f32) -> Self {
        Self { action, multiplier }
    }
}

pub struct KeyboardMap {
    bindings: Vec<(KeyCode, KeyMap)>,
}

impl Default for KeyboardMap {
    fn default() -> Self {
        Self::new()
    }
}

impl KeyboardMap {
    pub fn new() -> Self {
        Self {
            bindings: Default::default(),
        }
    }

    pub fn bind(mut self, key: KeyCode, map: KeyMap) -> Self {
        self.bindings.push((key, map));
        self
    }

    pub fn map(&mut self, keyboard: &KeyboardState) -> AHashMap<Action, f32> {
        let mut result: AHashMap<Action, f32> = AHashMap::new();

        for (key, s) in &mut self.bindings {
            let activation = if keyboard.is_down(*key) { 1.0 } else { 0.0 };
            *result.entry(s.action).or_default() += activation * s.multiplier;
        }

        for value in result.values_mut() {
            *value = value.clamp(-1.0, 1.0);
        }

        result
    }
}

#[derive(Debug, Default, Clone)]
pub struct Input {
    pub keyboard_state: KeyboardState,
    pub mouse_state: MouseState,
}

impl Input {
    pub fn new() -> Self {
        Default::default()
    }

    pub fn tick(&mut self) {
        self.keyboard_state.keys_down.values_mut().for_each(|val| {
            val.ticks = val.ticks.wrapping_add(1);
        });
    }

    pub fn on_device_event(&mut self, event: &DeviceEvent) {
        match event {
            DeviceEvent::MouseWheel { delta, .. } => {
                self.mouse_state.scroll = -match delta {
                    MouseScrollDelta::LineDelta(_, scroll) => *scroll,
                    MouseScrollDelta::PixelDelta(PhysicalPosition { y: scroll, .. }) => {
                        *scroll as f32
                    }
                };
            }
            DeviceEvent::MouseMotion { delta: (dx, dy) } => {
                self.mouse_state.delta = vec2(*dx as _, *dy as _);
            }
            _ => {}
        }
    }

    pub fn on_window_event(&mut self, window: &Window, event: &WindowEvent) {
        let mouse = &mut self.mouse_state;
        let keyb = &mut self.keyboard_state.keys_down;

        match event {
            WindowEvent::CursorMoved {
                position: PhysicalPosition { x, y },
                ..
            } => {
                let PhysicalSize { width, height } = window.inner_size();
                let x = (*x as f32 / width as f32 - 0.5) * 2.;
                let y = -(*y as f32 / height as f32 - 0.5) * 2.;
                mouse.screen_position = vec2(x, y);
            }
            WindowEvent::MouseInput { button, state, .. } => {
                let button_id = {
                    let button = match button {
                        MouseButton::Right => MouseState::RIGHT,
                        MouseButton::Middle => MouseState::MIDDLE,
                        MouseButton::Left => MouseState::LEFT,
                        _ => MouseState::LEFT,
                    };
                    1 << button
                };

                if let ElementState::Pressed = state {
                    mouse.buttons_held |= button_id;
                    mouse.buttons_pressed |= button_id;
                } else {
                    mouse.buttons_held &= !button_id;
                    mouse.buttons_released |= button_id;
                }
            }
            WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        physical_key: PhysicalKey::Code(keycode),
                        state,
                        ..
                    },
                ..
            } => {
                if state == &ElementState::Pressed {
                    keyb.entry(*keycode).or_insert(KeyState { ticks: 0 });
                } else {
                    keyb.remove(keycode);
                }
            }
            _ => {}
        }
    }
}
