use crate::NetworkUpdate;
use mmorpg_client::{ClientError, graphics::WindowRenderer, presentation::Presentation};
use mmorpg_core::ZoneDefinition;
use std::{
    collections::HashSet,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::watch;
use winit::{
    application::ApplicationHandler,
    event::{ElementState, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    keyboard::{KeyCode, PhysicalKey},
    window::{Window, WindowId},
};

pub fn run(
    runtime: Arc<tokio::runtime::Runtime>,
    player_id: u32,
    definition: ZoneDefinition,
    input: watch::Sender<[i8; 2]>,
    updates: watch::Receiver<NetworkUpdate>,
    frames: Option<u32>,
) -> Result<(), ClientError> {
    let event_loop = EventLoop::new()?;
    let mut app = App {
        runtime,
        window: None,
        renderer: None,
        presentation: Presentation::new(player_id, definition, Instant::now()),
        input,
        updates,
        keys: HashSet::new(),
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
    presentation: Presentation,
    input: watch::Sender<[i8; 2]>,
    updates: watch::Receiver<NetworkUpdate>,
    keys: HashSet<KeyCode>,
    error: Option<String>,
    frames: Option<u32>,
    rendered: u32,
    next_frame: Instant,
}

impl App {
    fn fail(&mut self, event_loop: &ActiveEventLoop, error: impl ToString) {
        self.input.send_replace([0, 0]);
        self.error = Some(error.to_string());
        event_loop.exit();
    }

    fn update_movement(&self) {
        let held = |first, second| self.keys.contains(&first) || self.keys.contains(&second);
        self.input.send_replace([
            i8::from(held(KeyCode::KeyD, KeyCode::ArrowRight))
                - i8::from(held(KeyCode::KeyA, KeyCode::ArrowLeft)),
            i8::from(held(KeyCode::KeyS, KeyCode::ArrowDown))
                - i8::from(held(KeyCode::KeyW, KeyCode::ArrowUp)),
        ]);
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
            .block_on(WindowRenderer::new(Arc::clone(&window)))
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
                self.input.send_replace([0, 0]);
                event_loop.exit();
            }
            WindowEvent::Resized(size) => {
                if let Some(renderer) = &mut self.renderer {
                    renderer.resize(size.width, size.height);
                }
            }
            WindowEvent::Focused(false) => {
                self.keys.clear();
                self.update_movement();
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if let PhysicalKey::Code(code) = event.physical_key {
                    if code == KeyCode::Escape && event.state == ElementState::Pressed {
                        event_loop.exit();
                        return;
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
                    NetworkUpdate::Snapshot(snapshot) => {
                        if let Err(error) = self.presentation.push((*snapshot).clone(), now) {
                            self.fail(event_loop, error);
                            return;
                        }
                        if let Some(window) = &self.window {
                            window.set_title(if self.presentation.is_stalled(now) {
                                "MMORPG — connection stalled"
                            } else {
                                "MMORPG — WASD / arrows move · Esc closes"
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
                        self.presentation.camera_target(now),
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
