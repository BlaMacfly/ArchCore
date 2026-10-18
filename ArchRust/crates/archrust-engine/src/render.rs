//! Contexte wgpu : surface, pipeline éclairé, maillages, textures, instances.

use anyhow::{anyhow, Result};
use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec3, Vec4};
use std::collections::HashMap;
use std::sync::Arc;
use wgpu::util::DeviceExt;
use winit::window::Window;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable, Debug)]
pub struct Vertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
    pub uv: [f32; 2],
    pub color: [f32; 4],
}

impl Vertex {
    const ATTRS: [wgpu::VertexAttribute; 4] =
        wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x2, 3 => Float32x4];
    fn layout() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Vertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &Self::ATTRS,
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Globals {
    view_proj: [[f32; 4]; 4],
    light_dir: [f32; 4],
    ambient: [f32; 4],
    camera_pos: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct InstanceUniform {
    model: [[f32; 4]; 4],
    color: [f32; 4],
}

/// Sous-maillage : plage d'indices + texture.
pub struct SubMesh {
    pub index_start: u32,
    pub index_count: u32,
    pub texture: Option<String>,
    pub color: [f32; 4],
}

pub struct Mesh {
    pub vertices: wgpu::Buffer,
    pub indices: wgpu::Buffer,
    pub parts: Vec<SubMesh>,
    pub bounds_center: Vec3,
    pub bounds_radius: f32,
}

pub struct GpuTexture {
    pub bind_group: wgpu::BindGroup,
    pub width: u32,
    pub height: u32,
}

/// Instance à dessiner : maillage + transformation.
pub struct DrawItem {
    pub mesh: Arc<Mesh>,
    pub model: Mat4,
    pub tint: [f32; 4],
}

pub struct Renderer {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub surface: wgpu::Surface<'static>,
    pub config: wgpu::SurfaceConfiguration,
    pipeline: wgpu::RenderPipeline,
    globals_buf: wgpu::Buffer,
    globals_bg: wgpu::BindGroup,
    texture_layout: wgpu::BindGroupLayout,
    instance_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    depth: wgpu::TextureView,
    pub textures: HashMap<String, Arc<GpuTexture>>,
    pub white: Arc<GpuTexture>,
    instance_pool: Vec<(wgpu::Buffer, wgpu::BindGroup)>,
    pub clear_color: wgpu::Color,
    /// Demande de capture de la prochaine image (lue dans `take_capture`).
    pub capture_next: bool,
    captured: Option<(u32, u32, Vec<u8>)>,
}

impl Renderer {
    pub fn new(window: Arc<Window>) -> Result<Self> {
        let size = window.inner_size();
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::PRIMARY,
            ..Default::default()
        });
        let surface = instance.create_surface(window.clone())?;
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
        }))
        .ok_or_else(|| anyhow!("aucun adaptateur graphique compatible"))?;
        log::info!(
            "GPU : {} ({:?})",
            adapter.get_info().name,
            adapter.get_info().backend
        );
        let (device, queue) = pollster::block_on(adapter.request_device(
            &wgpu::DeviceDescriptor {
                label: Some("archrust"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::default(),
                memory_hints: Default::default(),
            },
            None,
        ))?;
        let caps = surface.get_capabilities(&adapter);
        let format = caps
            .formats
            .iter()
            .copied()
            .find(|f| f.is_srgb())
            .unwrap_or(caps.formats[0]);
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode: wgpu::PresentMode::AutoVsync,
            alpha_mode: caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &config);

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("lit"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shader.wgsl").into()),
        });
        let globals_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("globals"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let texture_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("texture"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let instance_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("instance"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("lit"),
            bind_group_layouts: &[&globals_layout, &texture_layout, &instance_layout],
            push_constant_ranges: &[],
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("lit"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[Vertex::layout()],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: true,
                depth_compare: wgpu::CompareFunction::Less,
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: Default::default(),
            multiview: None,
            cache: None,
        });
        let globals_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("globals"),
            size: std::mem::size_of::<Globals>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let globals_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("globals"),
            layout: &globals_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: globals_buf.as_entire_binding(),
            }],
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Linear,
            anisotropy_clamp: 8,
            ..Default::default()
        });
        let depth = Self::make_depth(&device, &config);
        let white = Arc::new(upload_rgba(
            &device,
            &queue,
            &texture_layout,
            &sampler,
            "__white",
            1,
            1,
            &[255, 255, 255, 255],
        ));
        let mut textures = HashMap::new();
        textures.insert("__white".to_string(), white.clone());
        Ok(Renderer {
            device,
            queue,
            surface,
            config,
            pipeline,
            globals_buf,
            globals_bg,
            texture_layout,
            instance_layout,
            sampler,
            depth,
            textures,
            white,
            instance_pool: Vec::new(),
            clear_color: wgpu::Color {
                r: 0.35,
                g: 0.45,
                b: 0.6,
                a: 1.0,
            },
            capture_next: false,
            captured: None,
        })
    }

    /// Image capturée (RGBA8) si une capture a été demandée puis rendue.
    pub fn take_capture(&mut self) -> Option<(u32, u32, Vec<u8>)> {
        self.captured.take()
    }

    /// Copie la surface rendue vers un tampon lisible par le CPU.
    fn capture(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        texture: &wgpu::Texture,
    ) -> (wgpu::Buffer, u32) {
        let (w, h) = (self.config.width, self.config.height);
        let bytes_per_row = (4 * w).div_ceil(256) * 256;
        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("capture"),
            size: bytes_per_row as u64 * h as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(bytes_per_row),
                    rows_per_image: Some(h),
                },
            },
            wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
        );
        (buffer, bytes_per_row)
    }

    fn make_depth(device: &wgpu::Device, config: &wgpu::SurfaceConfiguration) -> wgpu::TextureView {
        let tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("depth"),
            size: wgpu::Extent3d {
                width: config.width.max(1),
                height: config.height.max(1),
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Depth32Float,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        tex.create_view(&Default::default())
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }
        self.config.width = width;
        self.config.height = height;
        self.surface.configure(&self.device, &self.config);
        self.depth = Self::make_depth(&self.device, &self.config);
    }

    pub fn aspect(&self) -> f32 {
        self.config.width as f32 / self.config.height.max(1) as f32
    }

    /// Téléverse une texture RGBA8 (un seul niveau) et l'enregistre sous `name`.
    pub fn upload_texture(
        &mut self,
        name: &str,
        width: u32,
        height: u32,
        rgba: &[u8],
    ) -> Arc<GpuTexture> {
        let gpu = Arc::new(upload_rgba(
            &self.device,
            &self.queue,
            &self.texture_layout,
            &self.sampler,
            name,
            width,
            height,
            rgba,
        ));
        self.textures.insert(name.to_ascii_lowercase(), gpu.clone());
        gpu
    }

    pub fn texture(&self, name: &str) -> Option<Arc<GpuTexture>> {
        self.textures.get(&name.to_ascii_lowercase()).cloned()
    }

    pub fn create_mesh(&self, vertices: &[Vertex], indices: &[u32], parts: Vec<SubMesh>) -> Mesh {
        let (center, radius) = bounds(vertices);
        Mesh {
            vertices: self
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("vertices"),
                    contents: bytemuck::cast_slice(vertices),
                    usage: wgpu::BufferUsages::VERTEX,
                }),
            indices: self
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("indices"),
                    contents: bytemuck::cast_slice(indices),
                    usage: wgpu::BufferUsages::INDEX,
                }),
            parts,
            bounds_center: center,
            bounds_radius: radius,
        }
    }

    fn ensure_instances(&mut self, count: usize) {
        while self.instance_pool.len() < count {
            let buf = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("instance"),
                size: std::mem::size_of::<InstanceUniform>() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            let bg = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("instance"),
                layout: &self.instance_layout,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: buf.as_entire_binding(),
                }],
            });
            self.instance_pool.push((buf, bg));
        }
    }

    /// Dessine une image : scène 3D puis une fermeture pour l'interface (egui) sur le même encodeur.
    pub fn render(
        &mut self,
        view_proj: Mat4,
        camera_pos: Vec3,
        items: &[DrawItem],
        ui: impl FnOnce(&wgpu::Device, &wgpu::Queue, &mut wgpu::CommandEncoder, &wgpu::TextureView),
    ) -> Result<()> {
        let frame = match self.surface.get_current_texture() {
            Ok(f) => f,
            Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                self.surface.configure(&self.device, &self.config);
                return Ok(());
            }
            Err(e) => return Err(e.into()),
        };
        let view = frame.texture.create_view(&Default::default());
        let globals = Globals {
            view_proj: view_proj.to_cols_array_2d(),
            light_dir: Vec4::new(0.4, 0.8, 0.3, 0.0).to_array(),
            ambient: [0.45, 0.45, 0.5, 1.0],
            camera_pos: camera_pos.extend(1.0).to_array(),
        };
        self.queue
            .write_buffer(&self.globals_buf, 0, bytemuck::bytes_of(&globals));
        self.ensure_instances(items.len());
        for (i, item) in items.iter().enumerate() {
            let data = InstanceUniform {
                model: item.model.to_cols_array_2d(),
                color: item.tint,
            };
            let (buf, _) = &self.instance_pool[i];
            self.queue.write_buffer(buf, 0, bytemuck::bytes_of(&data));
        }
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("frame"),
            });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("scene"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(self.clear_color),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.depth,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &self.globals_bg, &[]);
            for (i, item) in items.iter().enumerate() {
                let (_, bg) = &self.instance_pool[i];
                pass.set_bind_group(2, bg, &[]);
                pass.set_vertex_buffer(0, item.mesh.vertices.slice(..));
                pass.set_index_buffer(item.mesh.indices.slice(..), wgpu::IndexFormat::Uint32);
                for part in &item.mesh.parts {
                    let tex = part
                        .texture
                        .as_ref()
                        .and_then(|t| self.textures.get(&t.to_ascii_lowercase()))
                        .unwrap_or(&self.white);
                    pass.set_bind_group(1, &tex.bind_group, &[]);
                    pass.draw_indexed(
                        part.index_start..part.index_start + part.index_count,
                        0,
                        0..1,
                    );
                }
            }
        }
        ui(&self.device, &self.queue, &mut encoder, &view);
        let capture = if self.capture_next {
            self.capture_next = false;
            Some(self.capture(&mut encoder, &frame.texture))
        } else {
            None
        };
        self.queue.submit(Some(encoder.finish()));
        frame.present();
        if let Some((buffer, bytes_per_row)) = capture {
            let (w, h) = (self.config.width, self.config.height);
            let slice = buffer.slice(..);
            let (tx, rx) = std::sync::mpsc::channel();
            slice.map_async(wgpu::MapMode::Read, move |r| {
                let _ = tx.send(r);
            });
            self.device.poll(wgpu::Maintain::Wait);
            if let Ok(Ok(())) = rx.recv() {
                let data = slice.get_mapped_range();
                let mut rgba = Vec::with_capacity((w * h * 4) as usize);
                let bgra = self.config.format == wgpu::TextureFormat::Bgra8UnormSrgb
                    || self.config.format == wgpu::TextureFormat::Bgra8Unorm;
                for row in 0..h {
                    let start = (row * bytes_per_row) as usize;
                    let line = &data[start..start + (w * 4) as usize];
                    if bgra {
                        for px in line.chunks_exact(4) {
                            rgba.extend_from_slice(&[px[2], px[1], px[0], 255]);
                        }
                    } else {
                        for px in line.chunks_exact(4) {
                            rgba.extend_from_slice(&[px[0], px[1], px[2], 255]);
                        }
                    }
                }
                drop(data);
                buffer.unmap();
                self.captured = Some((w, h, rgba));
            }
        }
        Ok(())
    }
}

#[allow(clippy::too_many_arguments)]
fn upload_rgba(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    layout: &wgpu::BindGroupLayout,
    sampler: &wgpu::Sampler,
    name: &str,
    width: u32,
    height: u32,
    rgba: &[u8],
) -> GpuTexture {
    let size = wgpu::Extent3d {
        width,
        height,
        depth_or_array_layers: 1,
    };
    let tex = device.create_texture(&wgpu::TextureDescriptor {
        label: Some(name),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &tex,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        rgba,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(4 * width),
            rows_per_image: Some(height),
        },
        size,
    );
    let view = tex.create_view(&Default::default());
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some(name),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
        ],
    });
    GpuTexture {
        bind_group,
        width,
        height,
    }
}

fn bounds(vertices: &[Vertex]) -> (Vec3, f32) {
    if vertices.is_empty() {
        return (Vec3::ZERO, 0.0);
    }
    let mut min = Vec3::splat(f32::MAX);
    let mut max = Vec3::splat(f32::MIN);
    for v in vertices {
        let p = Vec3::from_array(v.position);
        min = min.min(p);
        max = max.max(p);
    }
    let center = (min + max) * 0.5;
    (center, (max - center).length())
}
