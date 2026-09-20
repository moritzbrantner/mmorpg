//! Native GPU adapter for shared 3d-lab geometry/camera models.
use crate::{ClientError, presentation::SceneBox};
use bytemuck::{Pod, Zeroable};
use std::{mem, sync::Arc, time::Duration};
use three_d_camera::PerspectiveCamera;
use three_d_core::{Mesh, Vec3};
use wgpu::util::DeviceExt;
use winit::window::Window;

const MAX_INSTANCES: usize = mmorpg_core::MAX_STATIC_COLLIDERS + mmorpg_core::MAX_PLAYERS_PER_ZONE;
const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Vertex {
    position: [f32; 3],
    normal: [f32; 3],
}
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Instance {
    position: [f32; 3],
    size: [f32; 3],
    color: [f32; 3],
}

pub struct SceneRenderer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    pipeline: wgpu::RenderPipeline,
    vertices: wgpu::Buffer,
    vertex_count: u32,
    instances: wgpu::Buffer,
    camera: wgpu::Buffer,
    camera_bind_group: wgpu::BindGroup,
    depth: wgpu::TextureView,
    width: u32,
    height: u32,
}

impl SceneRenderer {
    async fn new(
        adapter: &wgpu::Adapter,
        format: wgpu::TextureFormat,
        width: u32,
        height: u32,
    ) -> Result<Self, ClientError> {
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("MMORPG client"),
                ..Default::default()
            })
            .await?;
        let mesh = Mesh::unit_cube();
        let mut vertices = Vec::new();
        for (triangle, indices) in mesh.indices().as_chunks::<3>().0.iter().enumerate() {
            let normal = mesh
                .triangle_normal(triangle)
                .ok_or("cube triangle has no normal")?;
            for index in indices {
                let vertex = mesh.vertices()[*index as usize];
                vertices.push(Vertex {
                    position: [vertex.x, vertex.y, vertex.z],
                    normal: [normal.x, normal.y, normal.z],
                });
            }
        }
        let vertex_count = u32::try_from(vertices.len())?;
        let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("shared cube mesh"),
            contents: bytemuck::cast_slice(&vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let instances = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("bounded scene instances"),
            size: (MAX_INSTANCES * mem::size_of::<Instance>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let camera = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("camera"),
            size: 64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("outpost shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("scene.wgsl").into()),
        });
        const VERTEX_ATTRIBUTES: [wgpu::VertexAttribute; 2] =
            wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3];
        const INSTANCE_ATTRIBUTES: [wgpu::VertexAttribute; 3] =
            wgpu::vertex_attr_array![2 => Float32x3, 3 => Float32x3, 4 => Float32x3];
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("outpost pipeline"),
            layout: None,
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[
                    Some(wgpu::VertexBufferLayout {
                        array_stride: mem::size_of::<Vertex>() as u64,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: &VERTEX_ATTRIBUTES,
                    }),
                    Some(wgpu::VertexBufferLayout {
                        array_stride: mem::size_of::<Instance>() as u64,
                        step_mode: wgpu::VertexStepMode::Instance,
                        attributes: &INSTANCE_ATTRIBUTES,
                    }),
                ],
            },
            primitive: wgpu::PrimitiveState {
                cull_mode: Some(wgpu::Face::Back),
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::Less),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let camera_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("camera binding"),
            layout: &pipeline.get_bind_group_layout(0),
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: camera.as_entire_binding(),
            }],
        });
        let depth = depth_texture(&device, width, height);
        Ok(Self {
            device,
            queue,
            pipeline,
            vertices: vertex_buffer,
            vertex_count,
            instances,
            camera,
            camera_bind_group,
            depth,
            width,
            height,
        })
    }

    fn render(
        &self,
        target: &wgpu::TextureView,
        scene: &[SceneBox],
        focus: [f32; 3],
    ) -> Result<(), ClientError> {
        if scene.len() > MAX_INSTANCES {
            return Err("scene exceeds instance capacity".into());
        }
        let instances: Vec<_> = scene
            .iter()
            .map(|item| Instance {
                position: item.position,
                size: item.size,
                color: item.color,
            })
            .collect();
        let camera = PerspectiveCamera::new(
            Vec3::new(focus[0] + 9.0, focus[1] + 10.0, focus[2] + 12.0),
            Vec3::new(focus[0], focus[1], focus[2]),
            Vec3::new(0.0, 1.0, 0.0),
            50_f32.to_radians(),
            self.width as f32 / self.height as f32,
            0.1,
            250.0,
        )?;
        self.queue.write_buffer(
            &self.camera,
            0,
            bytemuck::cast_slice(&camera.view_projection_matrix().elements),
        );
        self.queue
            .write_buffer(&self.instances, 0, bytemuck::cast_slice(&instances));
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("scene frame"),
            });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("opaque world"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.16,
                            g: 0.22,
                            b: 0.28,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.depth,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Discard,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &self.camera_bind_group, &[]);
            pass.set_vertex_buffer(0, self.vertices.slice(..));
            pass.set_vertex_buffer(1, self.instances.slice(..));
            pass.draw(0..self.vertex_count, 0..u32::try_from(scene.len())?);
        }
        self.queue.submit([encoder.finish()]);
        Ok(())
    }
}

fn depth_texture(device: &wgpu::Device, width: u32, height: u32) -> wgpu::TextureView {
    device
        .create_texture(&wgpu::TextureDescriptor {
            label: Some("depth"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: DEPTH_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        })
        .create_view(&Default::default())
}

pub struct WindowRenderer {
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    renderer: SceneRenderer,
    suspended: bool,
}

impl WindowRenderer {
    pub async fn new(window: Arc<Window>) -> Result<Self, ClientError> {
        let size = window.inner_size();
        let instance = wgpu::Instance::default();
        let surface = instance.create_surface(window)?;
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                compatible_surface: Some(&surface),
                ..Default::default()
            })
            .await?;
        let mut config = surface
            .get_default_config(&adapter, size.width.max(1), size.height.max(1))
            .ok_or("surface has no compatible format")?;
        config.present_mode = wgpu::PresentMode::AutoVsync;
        let renderer =
            SceneRenderer::new(&adapter, config.format, config.width, config.height).await?;
        surface.configure(&renderer.device, &config);
        Ok(Self {
            surface,
            config,
            renderer,
            suspended: false,
        })
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        self.suspended = width == 0 || height == 0;
        if self.suspended {
            return;
        }
        let limit = self.renderer.device.limits().max_texture_dimension_2d;
        self.config.width = width.min(limit);
        self.config.height = height.min(limit);
        self.renderer.width = self.config.width;
        self.renderer.height = self.config.height;
        self.renderer.depth =
            depth_texture(&self.renderer.device, self.config.width, self.config.height);
        self.surface.configure(&self.renderer.device, &self.config);
    }

    pub fn render(&mut self, scene: &[SceneBox], focus: [f32; 3]) -> Result<(), ClientError> {
        if self.suspended {
            return Ok(());
        }
        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(frame)
            | wgpu::CurrentSurfaceTexture::Suboptimal(frame) => frame,
            wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => {
                return Ok(());
            }
            wgpu::CurrentSurfaceTexture::Outdated => {
                self.resize(self.config.width, self.config.height);
                return Ok(());
            }
            wgpu::CurrentSurfaceTexture::Lost => {
                return Err("GPU surface lost; restart the client".into());
            }
            wgpu::CurrentSurfaceTexture::Validation => {
                return Err("GPU surface validation failed".into());
            }
        };
        self.renderer.render(
            &frame.texture.create_view(&Default::default()),
            scene,
            focus,
        )?;
        self.renderer.queue.present(frame);
        Ok(())
    }
}

/// Explicit GPU smoke check. Readback proves a frame was rendered, rather than
/// accepting successful command submission as graphics evidence.
pub async fn render_offscreen(scene: &[SceneBox], focus: [f32; 3]) -> Result<usize, ClientError> {
    const WIDTH: u32 = 640;
    const HEIGHT: u32 = 360;
    let instance = wgpu::Instance::default();
    let adapter = instance.request_adapter(&Default::default()).await?;
    let renderer =
        SceneRenderer::new(&adapter, wgpu::TextureFormat::Rgba8UnormSrgb, WIDTH, HEIGHT).await?;
    let texture = renderer.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("smoke target"),
        size: wgpu::Extent3d {
            width: WIDTH,
            height: HEIGHT,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    renderer.render(&texture.create_view(&Default::default()), scene, focus)?;
    let buffer = renderer.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("smoke readback"),
        size: u64::from(WIDTH * HEIGHT * 4),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = renderer.device.create_command_encoder(&Default::default());
    encoder.copy_texture_to_buffer(
        texture.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(WIDTH * 4),
                rows_per_image: Some(HEIGHT),
            },
        },
        wgpu::Extent3d {
            width: WIDTH,
            height: HEIGHT,
            depth_or_array_layers: 1,
        },
    );
    renderer.queue.submit([encoder.finish()]);
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    buffer
        .slice(..)
        .map_async(wgpu::MapMode::Read, move |result| {
            let _ = sender.send(result);
        });
    renderer.device.poll(wgpu::PollType::Wait {
        submission_index: None,
        timeout: Some(Duration::from_secs(10)),
    })?;
    receiver.recv_timeout(Duration::from_secs(10))??;
    let bytes = buffer.slice(..).get_mapped_range()?;
    let colors = bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|pixel| [pixel[0], pixel[1], pixel[2]])
        .collect::<std::collections::BTreeSet<_>>();
    let count = colors.len();
    drop(bytes);
    buffer.unmap();
    if count < 3 {
        return Err("GPU frame did not contain visible scene geometry".into());
    }
    Ok(count)
}
