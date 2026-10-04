use crate::game::Game;
use bytemuck::{Pod, Zeroable};
use eframe::egui;
use glam::{Mat4, Vec3, Vec4};
use shakmaty::{Color, Move, Role, Square};
use wgpu::util::DeviceExt;

#[derive(Clone, Copy, Pod, Zeroable)]
#[repr(C)]
struct Vertex {
    position: [f32; 3],
    normal: [f32; 3],
    uv: [f32; 2],
}
const PIECE_MESHES: [&[u8]; 6] = [
    include_bytes!("../assets/pawn.mesh"),
    include_bytes!("../assets/knight.mesh"),
    include_bytes!("../assets/bishop.mesh"),
    include_bytes!("../assets/rook.mesh"),
    include_bytes!("../assets/queen.mesh"),
    include_bytes!("../assets/king.mesh"),
];

fn piece_vertices(bytes: &[u8]) -> impl Iterator<Item = Vertex> + '_ {
    assert_eq!(bytes.len() % (3 * std::mem::size_of::<Vertex>()), 0);
    bytes.as_chunks::<32>().0.iter().map(|chunk| {
        let words = chunk.as_chunks::<4>().0;
        let f: [f32; 8] = std::array::from_fn(|i| f32::from_le_bytes(words[i]));
        Vertex {
            position: f[0..3].try_into().unwrap(),
            normal: f[3..6].try_into().unwrap(),
            uv: f[6..8].try_into().unwrap(),
        }
    })
}
#[derive(Clone, Copy, Pod, Zeroable)]
#[repr(C)]
struct Instance {
    model: [[f32; 4]; 4],
    tint: [f32; 4],
    params: [f32; 4],
    material: [f32; 4],
}
#[derive(Clone, Copy, Pod, Zeroable)]
#[repr(C)]
struct Camera {
    vp: [[f32; 4]; 4],
    eye: [f32; 4],
    background: [f32; 4],
    light: [f32; 4],
    lighting: [f32; 4],
    viewport: [f32; 4],
}
#[derive(Clone, Copy)]
struct Mesh {
    start: u32,
    count: u32,
}
#[derive(Clone, Copy, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Material {
    pub diffuse: f32,
    pub specular: f32,
    pub shininess: f32,
    pub alpha: f32,
}
impl Default for Material {
    fn default() -> Self {
        Self {
            diffuse: 0.68,
            specular: 0.55,
            shininess: 35.0,
            alpha: 1.0,
        }
    }
}
#[derive(Clone, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct View {
    pub yaw: f32,
    pub elevation: f32,
    pub distance: f32,
    pub board_style: usize,
    pub piece_style: usize,
    pub flat: bool,
    pub coordinates: bool,
    pub animations: bool,
    pub materials: [Material; 5],
    pub light: [f32; 3],
    pub ambient: f32,
    pub reflectivity: f32,
    pub label_intensity: f32,
}
impl Default for View {
    fn default() -> Self {
        Self {
            yaw: 0.0,
            elevation: 55.0,
            distance: 13.7,
            board_style: 0,
            piece_style: 0,
            flat: false,
            coordinates: true,
            animations: true,
            materials: [Material::default(); 5],
            light: [-6.0, 12.0, 8.0],
            ambient: 0.38,
            reflectivity: 0.25,
            label_intensity: 0.9,
        }
    }
}
impl View {
    pub fn matrices(&self, aspect: f32) -> (Mat4, Vec3) {
        let yaw = self.yaw.to_radians();
        let tilt = self.elevation.to_radians();
        let eye =
            Vec3::new(yaw.sin() * tilt.cos(), tilt.sin(), yaw.cos() * tilt.cos()) * self.distance;
        let vp = Mat4::perspective_rh(40.0f32.to_radians(), aspect, 0.1, 100.0)
            * Mat4::look_at_rh(eye, Vec3::new(0.0, 0.3, 0.0), Vec3::Y);
        (vp, eye)
    }
    pub fn project(&self, point: Vec3, rect: egui::Rect) -> Option<egui::Pos2> {
        let (vp, _) = self.matrices(rect.aspect_ratio());
        let p = vp * point.extend(1.0);
        if p.w <= 0.0 {
            return None;
        }
        let p = p.truncate() / p.w;
        Some(egui::pos2(
            rect.left() + (p.x + 1.0) * 0.5 * rect.width(),
            rect.top() + (1.0 - p.y) * 0.5 * rect.height(),
        ))
    }
    pub fn pick(&self, p: egui::Pos2, rect: egui::Rect, g: &Game) -> Option<Square> {
        let (vp, _) = self.matrices(rect.aspect_ratio());
        let inv = vp.inverse();
        let x = 2.0 * (p.x - rect.left()) / rect.width() - 1.0;
        let y = 1.0 - 2.0 * (p.y - rect.top()) / rect.height();
        let near = inv * Vec4::new(x, y, 0.0, 1.0);
        let far = inv * Vec4::new(x, y, 1.0, 1.0);
        let a = near.truncate() / near.w;
        let b = far.truncate() / far.w;
        let ray = (b - a).normalize();
        let mut nearest = f32::MAX;
        let mut picked = None;
        // Ray/sphere selection around each piece's body makes clicks on elevated pieces intuitive.
        for (sq, piece) in g.board.pos.board().iter() {
            let height = match piece.role {
                Role::Pawn => 1.0,
                Role::Rook => 1.1,
                Role::Knight | Role::Bishop => 1.5,
                _ => 1.8,
            };
            let center = square_world(sq) + Vec3::Y * height * 0.45;
            let delta = a - center;
            let bb = delta.dot(ray);
            let cc = delta.length_squared() - 0.34 * 0.34;
            let disc = bb * bb - cc;
            if disc >= 0.0 {
                let t = -bb - disc.sqrt();
                if t > 0.0 && t < nearest {
                    nearest = t;
                    picked = Some(sq);
                }
            }
        }
        if picked.is_some() {
            return picked;
        }
        if ray.y.abs() < 0.001 {
            return None;
        }
        let t = -a.y / ray.y;
        if t < 0.0 {
            return None;
        }
        let hit = a + ray * t;
        let file = (hit.x + 4.0).floor() as i32;
        let rank = (4.0 - hit.z).floor() as i32;
        if (0..8).contains(&file) && (0..8).contains(&rank) {
            Some(Square::new((rank * 8 + file) as u32))
        } else {
            None
        }
    }
}
use shakmaty::Position;
pub fn square_world(sq: Square) -> Vec3 {
    Vec3::new(
        i32::from(sq.file()) as f32 - 3.5,
        0.0,
        3.5 - i32::from(sq.rank()) as f32,
    )
}
pub fn destination(m: Move) -> Square {
    if let Move::Castle { king, rook } = m {
        Square::new(u32::from(king.rank()) * 8 + if rook.file() > king.file() { 6 } else { 2 })
    } else {
        m.to()
    }
}
pub struct Motion {
    pub m: Move,
    pub previous: Game,
    pub progress: f32,
}

pub struct BoardRenderer {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pipeline: wgpu::RenderPipeline,
    uniform: wgpu::Buffer,
    bind: wgpu::BindGroup,
    reflection_bind: wgpu::BindGroup,
    layout: wgpu::BindGroupLayout,
    materials_view: wgpu::TextureView,
    sampler: wgpu::Sampler,
    reflection: wgpu::TextureView,
    vertices: wgpu::Buffer,
    instances: wgpu::Buffer,
    meshes: Vec<Mesh>,
    pub target: wgpu::Texture,
    view: wgpu::TextureView,
    depth: wgpu::TextureView,
    msaa: wgpu::TextureView,
    pub size: [u32; 2],
    pub texture_id: Option<egui::TextureId>,
    pub adapter_name: String,
}
impl BoardRenderer {
    pub fn new(device: wgpu::Device, queue: wgpu::Queue, adapter_name: String) -> Self {
        let mut vertices = Vec::new();
        let mut meshes = Vec::new();
        for bytes in PIECE_MESHES {
            let start = vertices.len() as u32;
            vertices.extend(piece_vertices(bytes));
            meshes.push(Mesh {
                start,
                count: vertices.len() as u32 - start,
            });
        }
        let start = vertices.len() as u32;
        plane(&mut vertices);
        meshes.push(Mesh { start, count: 6 });
        let start = vertices.len() as u32;
        cube(&mut vertices);
        meshes.push(Mesh { start, count: 36 });
        let start = vertices.len() as u32;
        for i in 0..48 {
            let a = i as f32 * std::f32::consts::TAU / 48.0;
            let b = (i + 1) as f32 * std::f32::consts::TAU / 48.0;
            for p in [
                [0.0, 0.0, 0.0],
                [a.cos(), 0.0, a.sin()],
                [b.cos(), 0.0, b.sin()],
            ] {
                vertices.push(Vertex {
                    position: p,
                    normal: [0.0, 1.0, 0.0],
                    uv: [0.0, 0.0],
                });
            }
        }
        meshes.push(Mesh { start, count: 144 });
        let vertices = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Original chess geometry"),
            contents: bytemuck::cast_slice(&vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let instances = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Scene instances"),
            size: 1024 * std::mem::size_of::<Instance>() as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Camera"),
            size: std::mem::size_of::<Camera>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let textures = texture_bytes();
        let tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Original board and piece materials"),
            size: wgpu::Extent3d {
                width: 256,
                height: 256,
                depth_or_array_layers: textures.len() as u32,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        for (layer, bytes) in textures.iter().enumerate() {
            let img = image::load_from_memory(bytes)
                .expect("Embedded material is valid")
                .resize_exact(256, 256, image::imageops::FilterType::Triangle)
                .to_rgba8();
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &tex,
                    mip_level: 0,
                    origin: wgpu::Origin3d {
                        x: 0,
                        y: 0,
                        z: layer as u32,
                    },
                    aspect: wgpu::TextureAspect::All,
                },
                &img,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(1024),
                    rows_per_image: Some(256),
                },
                wgpu::Extent3d {
                    width: 256,
                    height: 256,
                    depth_or_array_layers: 1,
                },
            );
        }
        let texture_view = tex.create_view(&wgpu::TextureViewDescriptor {
            dimension: Some(wgpu::TextureViewDimension::D2Array),
            ..Default::default()
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Chess scene layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2Array,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
            ],
        });
        let (target, view, depth, msaa, reflection) = targets(&device, [1024, 768]);
        let dummy = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Empty reflection"),
            size: wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &dummy,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &[0, 0, 0, 0],
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4),
                rows_per_image: Some(1),
            },
            wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
        );
        let dummy_view = dummy.create_view(&Default::default());
        let bind = scene_bind(
            &device,
            &layout,
            &uniform,
            &texture_view,
            &sampler,
            &reflection,
        );
        let reflection_bind = scene_bind(
            &device,
            &layout,
            &uniform,
            &texture_view,
            &sampler,
            &dummy_view,
        );
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Chess Vulkan shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("board.wgsl").into()),
        });
        const VA: [wgpu::VertexAttribute; 3] =
            wgpu::vertex_attr_array![0=>Float32x3,1=>Float32x3,2=>Float32x2];
        const IA: [wgpu::VertexAttribute; 7] = wgpu::vertex_attr_array![3=>Float32x4,4=>Float32x4,5=>Float32x4,6=>Float32x4,7=>Float32x4,8=>Float32x4,9=>Float32x4];
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Chess 3D pipeline"),
            layout: Some(
                &device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                    label: None,
                    bind_group_layouts: &[&layout],
                    push_constant_ranges: &[],
                }),
            ),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[
                    wgpu::VertexBufferLayout {
                        array_stride: 32,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: &VA,
                    },
                    wgpu::VertexBufferLayout {
                        array_stride: 112,
                        step_mode: wgpu::VertexStepMode::Instance,
                        attributes: &IA,
                    },
                ],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: wgpu::TextureFormat::Rgba8Unorm,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: true,
                depth_compare: wgpu::CompareFunction::LessEqual,
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: wgpu::MultisampleState {
                count: 4,
                ..Default::default()
            },
            multiview: None,
            cache: None,
        });
        Self {
            device,
            queue,
            pipeline,
            uniform,
            bind,
            reflection_bind,
            layout,
            materials_view: texture_view,
            sampler,
            reflection,
            vertices,
            instances,
            meshes,
            target,
            view,
            depth,
            msaa,
            size: [1024, 768],
            texture_id: None,
            adapter_name,
        }
    }
    pub fn resize(&mut self, size: [u32; 2], state: &eframe::egui_wgpu::RenderState) {
        let size = [size[0].clamp(64, 4096), size[1].clamp(64, 4096)];
        if self.size != size {
            (
                self.target,
                self.view,
                self.depth,
                self.msaa,
                self.reflection,
            ) = targets(&self.device, size);
            self.bind = scene_bind(
                &self.device,
                &self.layout,
                &self.uniform,
                &self.materials_view,
                &self.sampler,
                &self.reflection,
            );
            self.size = size;
            if let Some(id) = self.texture_id {
                state
                    .renderer
                    .write()
                    .update_egui_texture_from_wgpu_texture(
                        &self.device,
                        &self.view,
                        wgpu::FilterMode::Linear,
                        id,
                    );
            }
        }
        if self.texture_id.is_none() {
            self.texture_id = Some(state.renderer.write().register_native_texture(
                &self.device,
                &self.view,
                wgpu::FilterMode::Linear,
            ));
        }
    }
    pub fn render(
        &mut self,
        g: &Game,
        v: &View,
        selected: Option<Square>,
        hint: Option<Move>,
        last: Option<Move>,
        motion: Option<&Motion>,
    ) {
        let (vp, eye) = v.matrices(self.size[0] as f32 / self.size[1] as f32);
        let camera = Camera {
            vp: vp.to_cols_array_2d(),
            eye: eye.extend(1.0).to_array(),
            background: [0.055, 0.065, 0.077, 1.0],
            light: [v.light[0], v.light[1], v.light[2], 1.0],
            lighting: [v.ambient, v.reflectivity, v.label_intensity, 0.0],
            viewport: [self.size[0] as f32, self.size[1] as f32, 0.0, 0.0],
        };
        self.queue
            .write_buffer(&self.uniform, 0, bytemuck::bytes_of(&camera));
        let mut batches: Vec<Vec<Instance>> = vec![vec![]; self.meshes.len()];
        let mut add = |mesh: usize,
                       pos: Vec3,
                       scale: Vec3,
                       yaw: f32,
                       tint: [f32; 4],
                       layer: f32,
                       rough: f32,
                       unlit: bool| {
            batches[mesh].push(Instance {
                model: (Mat4::from_translation(pos)
                    * Mat4::from_rotation_y(yaw)
                    * Mat4::from_scale(scale))
                .to_cols_array_2d(),
                tint,
                params: [
                    layer,
                    rough,
                    if unlit { 1.0 } else { 0.0 },
                    if mesh == 6 && !unlit { 1.0 } else { 0.0 },
                ],
                material: if unlit {
                    [1.0, 0.0, 1.0, 1.0]
                } else {
                    let index = if layer < 12.0 {
                        2 + layer as usize % 3
                    } else {
                        (layer as usize - 12) % 2
                    };
                    let style = v.materials[index];
                    [style.diffuse, style.specular, style.shininess, style.alpha]
                },
            });
        };
        let board = v.board_style.min(3) * 3;
        add(
            7,
            Vec3::new(0.0, -0.22, 0.0),
            Vec3::new(9.1, 0.42, 9.1),
            0.0,
            [0.55, 0.55, 0.55, 1.0],
            (board + 2) as f32,
            0.4,
            false,
        );
        for sq in Square::ALL {
            let p = square_world(sq);
            let dark = (i32::from(sq.rank()) + i32::from(sq.file())) % 2 == 0;
            add(
                6,
                p,
                Vec3::ONE,
                0.0,
                [1.0; 4],
                (board + usize::from(dark)) as f32,
                0.4,
                false,
            );
        }
        let legal = g.board.moves();
        if let Some(sq) = selected {
            add(
                6,
                square_world(sq) + Vec3::Y * 0.009,
                Vec3::splat(0.98),
                0.0,
                [0.9, 0.7, 0.25, 0.32],
                -1.0,
                1.0,
                true,
            );
            for m in legal.iter().filter(|m| m.from() == Some(sq)) {
                add(
                    8,
                    square_world(destination(*m)) + Vec3::Y * 0.014,
                    Vec3::splat(if m.is_capture() { 0.43 } else { 0.11 }),
                    0.0,
                    [0.8, 0.85, 0.65, 0.45],
                    -1.0,
                    1.0,
                    true,
                );
            }
        }
        for (m, color) in [
            (last, [0.85, 0.75, 0.35, 0.58]),
            (hint, [0.4, 0.85, 0.65, 0.8]),
        ] {
            if let Some(m) = m
                && let Some(from) = m.from()
            {
                let a = square_world(from);
                let b = square_world(destination(m));
                let delta = b - a;
                let angle = delta.x.atan2(delta.z);
                let len = delta.length();
                add(
                    6,
                    (a + b) * 0.5 + Vec3::Y * 0.025,
                    Vec3::new(0.1, 1.0, len),
                    angle,
                    color,
                    -1.0,
                    1.0,
                    true,
                );
                add(
                    6,
                    b + Vec3::Y * 0.028,
                    Vec3::new(0.26, 1.0, 0.26),
                    std::f32::consts::FRAC_PI_4,
                    color,
                    -1.0,
                    1.0,
                    true,
                );
            }
        }
        for (sq, mut piece) in g.board.pos.board().iter() {
            let mut pos = square_world(sq);
            if let Some(motion) = motion
                && sq == destination(motion.m)
                && let Some(from) = motion.m.from()
            {
                pos = square_world(from).lerp(pos, motion.progress);
                pos.y += (motion.progress * std::f32::consts::PI).sin() * 0.35;
                if motion.progress < 0.6
                    && let Some(old) = motion.previous.at(from)
                {
                    piece = old;
                }
            }
            if let Some(motion) = motion
                && let Move::Castle { king, rook } = motion.m
            {
                let target = Square::new(
                    u32::from(king.rank()) * 8 + if rook.file() > king.file() { 5 } else { 3 },
                );
                if sq == target {
                    pos = square_world(rook).lerp(square_world(target), motion.progress);
                }
            }
            add(
                8,
                pos + Vec3::new(0.07, 0.004, -0.06),
                Vec3::new(0.44, 1.0, 0.44),
                0.0,
                [0.0, 0.0, 0.0, 0.32],
                -1.0,
                1.0,
                true,
            );
            let mesh = match piece.role {
                Role::Pawn => 0,
                Role::Knight => 1,
                Role::Bishop => 2,
                Role::Rook => 3,
                Role::Queen => 4,
                Role::King => 5,
            };
            let layer = 12 + v.piece_style.min(3) * 2 + usize::from(piece.color == Color::Black);
            add(
                mesh,
                pos,
                Vec3::ONE,
                if (piece.role == Role::Knight && piece.color == Color::Black)
                    || (piece.role == Role::Bishop && piece.color == Color::White)
                {
                    std::f32::consts::PI
                } else {
                    0.0
                },
                [1.0; 4],
                layer as f32,
                if v.piece_style == 2 { 0.17 } else { 0.45 },
                false,
            );
        }
        let mut all = Vec::new();
        let mut draws = Vec::new();
        for (mesh, batch) in batches.iter().enumerate() {
            let start = all.len() as u32;
            all.extend_from_slice(batch);
            draws.push((mesh, start..all.len() as u32));
        }
        let mut reflection_draws = Vec::new();
        for (mesh, batch) in batches.iter().enumerate().take(6) {
            let start = all.len() as u32;
            for instance in batch {
                let mut reflected = *instance;
                reflected.model = (Mat4::from_scale(Vec3::new(1.0, -1.0, 1.0))
                    * Mat4::from_cols_array_2d(&instance.model))
                .to_cols_array_2d();
                all.push(reflected);
            }
            reflection_draws.push((mesh, start..all.len() as u32));
        }
        self.queue
            .write_buffer(&self.instances, 0, bytemuck::cast_slice(&all));
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Chess board frame"),
            });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Planar reflections"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.msaa,
                    depth_slice: None,
                    resolve_target: Some(&self.reflection),
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Discard,
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
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &self.reflection_bind, &[]);
            pass.set_vertex_buffer(0, self.vertices.slice(..));
            pass.set_vertex_buffer(1, self.instances.slice(..));
            for (index, instances) in reflection_draws {
                let mesh = self.meshes[index];
                pass.draw(mesh.start..mesh.start + mesh.count, instances);
            }
        }
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Chess board"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.msaa,
                    depth_slice: None,
                    resolve_target: Some(&self.view),
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.065,
                            g: 0.075,
                            b: 0.086,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Discard,
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
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &self.bind, &[]);
            pass.set_vertex_buffer(0, self.vertices.slice(..));
            pass.set_vertex_buffer(1, self.instances.slice(..));
            // Board and planar overlays precede opaque pieces, so transparent contact shadows blend correctly.
            for order in [7, 6, 8, 0, 1, 2, 3, 4, 5] {
                let (_, instances) = &draws[order];
                let mesh = self.meshes[order];
                pass.draw(mesh.start..mesh.start + mesh.count, instances.clone());
            }
        }
        self.queue.submit([encoder.finish()]);
    }
    pub fn save_png(&self, path: &std::path::Path) -> Result<(), String> {
        let row = (self.size[0] * 4).div_ceil(256) * 256;
        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Screenshot readback"),
            size: u64::from(row) * u64::from(self.size[1]),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = self.device.create_command_encoder(&Default::default());
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &self.target,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(row),
                    rows_per_image: Some(self.size[1]),
                },
            },
            wgpu::Extent3d {
                width: self.size[0],
                height: self.size[1],
                depth_or_array_layers: 1,
            },
        );
        let submission = self.queue.submit([encoder.finish()]);
        let (tx, rx) = std::sync::mpsc::channel();
        buffer.slice(..).map_async(wgpu::MapMode::Read, move |r| {
            let _ = tx.send(r);
        });
        self.device
            .poll(wgpu::PollType::Wait {
                submission_index: Some(submission),
                timeout: Some(std::time::Duration::from_secs(10)),
            })
            .map_err(|e| e.to_string())?;
        rx.recv()
            .map_err(|e| e.to_string())?
            .map_err(|e| e.to_string())?;
        let mapped = buffer.slice(..).get_mapped_range();
        let mut bytes = Vec::new();
        for line in mapped.chunks(row as usize) {
            bytes.extend_from_slice(&line[..self.size[0] as usize * 4]);
        }
        image::save_buffer(
            path,
            &bytes,
            self.size[0],
            self.size[1],
            image::ColorType::Rgba8,
        )
        .map_err(|e| e.to_string())
    }
}
fn targets(
    device: &wgpu::Device,
    size: [u32; 2],
) -> (
    wgpu::Texture,
    wgpu::TextureView,
    wgpu::TextureView,
    wgpu::TextureView,
    wgpu::TextureView,
) {
    let base = wgpu::TextureDescriptor {
        label: Some("Board render target"),
        size: wgpu::Extent3d {
            width: size[0],
            height: size[1],
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT
            | wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    };
    let reflection = device
        .create_texture(&base)
        .create_view(&Default::default());
    let target = device.create_texture(&base);
    let view = target.create_view(&Default::default());
    let msaa = device
        .create_texture(&wgpu::TextureDescriptor {
            sample_count: 4,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            ..base.clone()
        })
        .create_view(&Default::default());
    let depth = device
        .create_texture(&wgpu::TextureDescriptor {
            format: wgpu::TextureFormat::Depth32Float,
            sample_count: 4,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            ..base
        })
        .create_view(&Default::default());
    (target, view, depth, msaa, reflection)
}
fn plane(v: &mut Vec<Vertex>) {
    for (p, uv) in [
        ([-0.5, 0.0, -0.5], [0.0, 0.0]),
        ([0.5, 0.0, -0.5], [1.0, 0.0]),
        ([0.5, 0.0, 0.5], [1.0, 1.0]),
        ([-0.5, 0.0, -0.5], [0.0, 0.0]),
        ([0.5, 0.0, 0.5], [1.0, 1.0]),
        ([-0.5, 0.0, 0.5], [0.0, 1.0]),
    ] {
        v.push(Vertex {
            position: p,
            normal: [0.0, 1.0, 0.0],
            uv,
        });
    }
}
fn cube(v: &mut Vec<Vertex>) {
    let corners = [
        [-0.5, -0.5, -0.5],
        [0.5, -0.5, -0.5],
        [0.5, 0.5, -0.5],
        [-0.5, 0.5, -0.5],
        [-0.5, -0.5, 0.5],
        [0.5, -0.5, 0.5],
        [0.5, 0.5, 0.5],
        [-0.5, 0.5, 0.5],
    ];
    for (ids, n) in [
        ([0, 1, 2, 3], [0.0, 0.0, -1.0]),
        ([4, 5, 6, 7], [0.0, 0.0, 1.0]),
        ([0, 4, 7, 3], [-1.0, 0.0, 0.0]),
        ([1, 5, 6, 2], [1.0, 0.0, 0.0]),
        ([3, 2, 6, 7], [0.0, 1.0, 0.0]),
        ([0, 1, 5, 4], [0.0, -1.0, 0.0]),
    ] {
        for i in [0, 1, 2, 0, 2, 3] {
            v.push(Vertex {
                position: corners[ids[i]],
                normal: n,
                uv: [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]][i],
            });
        }
    }
}
fn texture_bytes() -> Vec<&'static [u8]> {
    macro_rules! asset {
        ($p:literal) => {
            include_bytes!(concat!("../Styles/", $p)).as_slice()
        };
    }
    vec![
        asset!("Wood/WhiteBoard.png"),
        asset!("Wood/BlackBoard.png"),
        asset!("Wood/Border.png"),
        asset!("Marble/WhiteBoard.png"),
        asset!("Marble/BlackBoard.png"),
        asset!("Marble/Border.png"),
        asset!("Metal/WhiteBoard.png"),
        asset!("Metal/BlackBoard.png"),
        asset!("Metal/Border.png"),
        asset!("Grass/WhiteBoard.png"),
        asset!("Grass/BlackBoard.png"),
        asset!("Grass/Border.png"),
        include_bytes!("../Resources/MTL/PiecesWhite/WhitePieceWood.jpg"),
        include_bytes!("../Resources/MTL/PiecesBlack/BlackPieceWood.jpg"),
        include_bytes!("../Resources/MTL/PiecesWhite/WhitePieceMarble.jpg"),
        include_bytes!("../Resources/MTL/PiecesBlack/BlackPieceMarble.jpg"),
        include_bytes!("../Resources/MTL/PiecesWhite/WhitePieceMetal.jpg"),
        include_bytes!("../Resources/MTL/PiecesBlack/BlackPieceMetal.jpg"),
        asset!("Fur/WhitePiece.png"),
        asset!("Fur/BlackPiece.png"),
    ]
}

fn scene_bind(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    uniform: &wgpu::Buffer,
    textures: &wgpu::TextureView,
    sampler: &wgpu::Sampler,
    reflection: &wgpu::TextureView,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("Scene material bindings"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(textures),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: wgpu::BindingResource::TextureView(reflection),
            },
        ],
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::Rules;
    #[test]
    fn piece_meshes_have_valid_shading_and_fit_board_squares() {
        for (bytes, height) in PIECE_MESHES
            .into_iter()
            .zip([1.11, 1.53, 1.62, 1.15, 1.90, 2.10])
        {
            let mut min = Vec3::splat(f32::INFINITY);
            let mut max = Vec3::splat(f32::NEG_INFINITY);
            for vertex in piece_vertices(bytes) {
                let position = Vec3::from_array(vertex.position);
                let normal = Vec3::from_array(vertex.normal);
                assert!(position.is_finite());
                assert!(normal.is_finite());
                assert!((normal.length() - 1.0).abs() < 0.001);
                // The supplied queen UVs extend slightly past the texture
                // edge; the repeating sampler handles this authored seam.
                assert!(
                    vertex
                        .uv
                        .into_iter()
                        .all(|uv| (-0.001..=1.001).contains(&uv))
                );
                min = min.min(position);
                max = max.max(position);
            }
            // Catches a lost stage rotation, incorrect model scale, or a
            // truncated conversion that drops a piece's head or base.
            assert!(min.x > -0.5 && max.x < 0.5);
            assert!(min.z > -0.5 && max.z < 0.5);
            assert!(min.y.abs() < 0.0001);
            assert!((max.y - height).abs() < 0.01);
        }
    }

    #[test]
    fn camera_projects_and_picks_both_sides() {
        let game = Game::new(Rules::Standard);
        let rect = egui::Rect::from_min_size(egui::pos2(0.0, 60.0), egui::vec2(1000.0, 730.0));
        for yaw in [0.0, 180.0] {
            let view = View {
                yaw,
                ..Default::default()
            };
            for sq in [
                Square::E2,
                Square::D2,
                Square::E7,
                Square::D7,
                Square::E4,
                Square::D5,
            ] {
                let point =
                    square_world(sq) + Vec3::Y * if game.at(sq).is_some() { 0.45 } else { 0.0 };
                let pixel = view.project(point, rect).unwrap();
                assert_eq!(view.pick(pixel, rect, &game), Some(sq), "{yaw} {sq}");
            }
        }
    }
}
