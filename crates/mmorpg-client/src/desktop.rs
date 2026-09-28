use mmorpg_client::session::{MovementInput, NetworkUpdate};
use mmorpg_client::{
    ClientError, camera::OrbitCamera, graphics::WindowRenderer, presentation::Presentation,
    world::WorldScene,
};
use mmorpg_scenery::Scenery;
use std::{
    collections::HashSet,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::watch;
use winit::{
    application::ApplicationHandler,
    dpi::PhysicalPosition,
    event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    keyboard::{KeyCode, PhysicalKey},
    window::{Window, WindowId},
};

pub fn run(
    runtime: Arc<tokio::runtime::Runtime>,
    player_id: u32,
    scenery: Scenery,
    world: WorldScene,
    input: watch::Sender<MovementInput>,
    updates: watch::Receiver<NetworkUpdate>,
    frames: Option<u32>,
) -> Result<(), ClientError> {
    let event_loop = EventLoop::new()?;
    let mut app = App {
        runtime,
        window: None,
        renderer: None,
        world,
        presentation: Presentation::new(player_id, scenery, Instant::now()),
        connection_epoch: None,
        input,
        updates,
        keys: HashSet::new(),
        camera: OrbitCamera::default(),
        orbit_buttons: HashSet::new(),
        cursor: None,
        error: None,
        frames,
        rendered: 0,
        next_frame: Instant::now(),
    };
    event_loop.run_app(&mut app)?;
    match app.error {
        Some(error) => Err(error.into()),
        None => Ok(()),
    }
}

struct App {
    runtime: Arc<tokio::runtime::Runtime>,
    window: Option<Arc<Window>>,
    renderer: Option<WindowRenderer>,
    /// Static geometry uploaded when the window's renderer is created.
    world: WorldScene,
    presentation: Presentation,
    connection_epoch: Option<u32>,
    input: watch::Sender<MovementInput>,
    updates: watch::Receiver<NetworkUpdate>,
    keys: HashSet<KeyCode>,
    camera: OrbitCamera,
    /// Held mouse buttons that orbit the camera while the cursor moves.
    orbit_buttons: HashSet<MouseButton>,
    cursor: Option<PhysicalPosition<f64>>,
    error: Option<String>,
    frames: Option<u32>,
    rendered: u32,
    next_frame: Instant,
}

const FORWARD_KEYS: [KeyCode; 2] = [KeyCode::KeyW, KeyCode::ArrowUp];
const BACKWARD_KEYS: [KeyCode; 2] = [KeyCode::KeyS, KeyCode::ArrowDown];
const LEFT_KEYS: [KeyCode; 3] = [KeyCode::KeyA, KeyCode::KeyQ, KeyCode::ArrowLeft];
const RIGHT_KEYS: [KeyCode; 3] = [KeyCode::KeyD, KeyCode::KeyE, KeyCode::ArrowRight];

/// Browser-style pixel scrolling: this many pixels count as one wheel line.
const PIXELS_PER_WHEEL_LINE: f64 = 40.0;
const CONTROL_HINTS: &str = "MMORPG — W/S move · A/D or Q/E strafe · Space jumps · drag to orbit · wheel zooms · Esc closes";

impl App {
    fn stop(&self) {
        self.input.send_if_modified(|input| {
            let moving = input.forward != 0 || input.strafe != 0;
            input.forward = 0;
            input.strafe = 0;
            moving
        });
    }

    fn fail(&mut self, event_loop: &ActiveEventLoop, error: impl ToString) {
        self.stop();
        self.error = Some(error.to_string());
        event_loop.exit();
    }

    /// While any movement key is held, the character faces the camera's yaw.
    fn update_movement(&self) {
        let held = |keys: &[KeyCode]| keys.iter().any(|key| self.keys.contains(key));
        let forward = i8::from(held(&FORWARD_KEYS)) - i8::from(held(&BACKWARD_KEYS));
        let strafe = i8::from(held(&RIGHT_KEYS)) - i8::from(held(&LEFT_KEYS));
        let steering =
            held(&FORWARD_KEYS) || held(&BACKWARD_KEYS) || held(&LEFT_KEYS) || held(&RIGHT_KEYS);
        let facing = steering.then(|| self.camera.facing());
        self.input.send_if_modified(|input| {
            let next = MovementInput {
                forward,
                strafe,
                facing: facing.unwrap_or(input.facing),
                jumps: input.jumps,
            };
            let changed = *input != next;
            *input = next;
            changed
        });
    }

    fn jump(&self) {
        self.input
            .send_modify(|input| input.jumps = input.jumps.wrapping_add(1));
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let window = match event_loop.create_window(
            Window::default_attributes()
                .with_title("MMORPG — connecting to world")
                .with_inner_size(winit::dpi::LogicalSize::new(1280, 800)),
        ) {
            Ok(window) => Arc::new(window),
            Err(error) => {
                self.fail(event_loop, error);
                return;
            }
        };
        match self
            .runtime
            .block_on(WindowRenderer::new(Arc::clone(&window), &self.world))
        {
            Ok(renderer) => {
                self.renderer = Some(renderer);
                self.window = Some(window);
            }
            Err(error) => self.fail(event_loop, error),
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::CloseRequested => {
                self.stop();
                event_loop.exit();
            }
            WindowEvent::Resized(size) => {
                if let Some(renderer) = &mut self.renderer {
                    renderer.resize(size.width, size.height);
                }
            }
            WindowEvent::Focused(false) => {
                self.keys.clear();
                self.orbit_buttons.clear();
                self.update_movement();
            }
            WindowEvent::MouseInput { state, button, .. } => {
                if matches!(button, MouseButton::Left | MouseButton::Right) {
                    match state {
                        ElementState::Pressed => self.orbit_buttons.insert(button),
                        ElementState::Released => self.orbit_buttons.remove(&button),
                    };
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                if let Some(previous) = self.cursor.replace(position)
                    && !self.orbit_buttons.is_empty()
                {
                    self.camera
                        .orbit(position.x - previous.x, position.y - previous.y);
                    // A held movement key keeps the character facing the camera.
                    self.update_movement();
                }
            }
            WindowEvent::CursorLeft { .. } => self.cursor = None,
            WindowEvent::MouseWheel { delta, .. } => {
                let lines = match delta {
                    MouseScrollDelta::LineDelta(_, lines) => f64::from(lines),
                    MouseScrollDelta::PixelDelta(position) => position.y / PIXELS_PER_WHEEL_LINE,
                };
                self.camera.zoom(lines as f32);
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if let PhysicalKey::Code(code) = event.physical_key {
                    if code == KeyCode::Escape && event.state == ElementState::Pressed {
                        event_loop.exit();
                        return;
                    }
                    // Jump is edge-triggered: key repeat never queues more jumps.
                    if code == KeyCode::Space
                        && event.state == ElementState::Pressed
                        && !event.repeat
                    {
                        self.jump();
                    }
                    if event.state == ElementState::Pressed {
                        self.keys.insert(code);
                    } else {
                        self.keys.remove(&code);
                    }
                    self.update_movement();
                }
            }
            WindowEvent::RedrawRequested => {
                let now = Instant::now();
                let update = self.updates.borrow_and_update().clone();
                match update {
                    NetworkUpdate::Waiting => {}
                    NetworkUpdate::Reconnecting => {
                        if let Some(window) = &self.window {
                            window.set_title("MMORPG — reconnecting to world");
                        }
                    }
                    NetworkUpdate::Snapshot {
                        connection_epoch,
                        snapshot,
                    } => {
                        if self.connection_epoch != Some(connection_epoch) {
                            self.presentation.reset(now);
                            self.connection_epoch = Some(connection_epoch);
                        }
                        if let Err(error) = self.presentation.push((*snapshot).clone(), now) {
                            self.fail(event_loop, error);
                            return;
                        }
                        if let Some(window) = &self.window {
                            window.set_title(if self.presentation.is_stalled(now) {
                                "MMORPG — connection stalled"
                            } else {
                                CONTROL_HINTS
                            });
                        }
                    }
                    NetworkUpdate::Failed(error) => {
                        self.fail(event_loop, error);
                        return;
                    }
                }
                if let Some(renderer) = &mut self.renderer
                    && let Err(error) = renderer.render(
                        &self.presentation.scene(now),
                        self.camera.view(self.presentation.camera_target(now)),
                    )
                {
                    self.fail(event_loop, error);
                    return;
                }
                self.rendered = self.rendered.saturating_add(1);
                if self.frames.is_some_and(|limit| self.rendered >= limit) {
                    event_loop.exit();
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let now = Instant::now();
        if now >= self.next_frame {
            if let Some(window) = &self.window {
                window.request_redraw();
            }
            self.next_frame = now + Duration::from_secs_f64(1.0 / 60.0);
        }
        event_loop.set_control_flow(ControlFlow::WaitUntil(self.next_frame));
    }
}
