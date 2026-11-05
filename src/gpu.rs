//! Custom wgpu paint callback embedded in egui. GPU fragment shader handles
//! ALL color adjustments; the CPU only converts MJPEG to RGBA and uploads it.
use std::{borrow::Cow, sync::Arc};
use egui_wgpu::{wgpu, CallbackResources, CallbackTrait};
use crate::capture::RgbaFrame;

#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
pub struct ColorSettings {
    pub exposure: f32,
    pub gamma: f32,
    pub highlights: f32,
    pub white: f32,
    pub saturation: f32,
}

impl Default for ColorSettings {
    fn default() -> Self {
        Self { exposure: 0.0, gamma: 1.0, highlights: 0.0, white: 1.0, saturation: 1.0 }
    }
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Zeroable, bytemuck::Pod)]
struct Uniforms { p0: [f32; 4], p1: [f32; 4] }

impl ColorSettings {
    fn uniforms(self, target_is_srgb: bool) -> Uniforms {
        Uniforms {
            p0: [self.exposure, self.gamma, self.highlights, self.white],
            p1: [self.saturation, if target_is_srgb { 1.0 } else { 0.0 }, 0.0, 0.0],
        }
    }
}

pub struct GpuVideo {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    uniform: wgpu::Buffer,
    texture: wgpu::Texture,
    group: wgpu::BindGroup,
    size: (u32, u32),
    target_is_srgb: bool,
}

fn create_video_texture(device: &wgpu::Device, w: u32, h: u32) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("NanoKVM MJPEG frame"),
        size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    })
}

fn create_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    texture: &wgpu::Texture,
    sampler: &wgpu::Sampler,
    uniform: &wgpu::Buffer,
) -> wgpu::BindGroup {
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("NanoKVM image / color controls"),
        layout,
        entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&view) },
            wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(sampler) },
            wgpu::BindGroupEntry { binding: 2, resource: uniform.as_entire_binding() },
        ],
    })
}

impl GpuVideo {
    pub fn new(device: &wgpu::Device, target: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("NanoKVM color correction WGSL"),
            source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(include_str!("color.wgsl"))),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("NanoKVM video layout"),
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
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("NanoKVM color pipeline layout"),
            bind_group_layouts: &[&layout],
            push_constant_ranges: &[],
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("NanoKVM video pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: target,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("NanoKVM video linear sample"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("NanoKVM shader controls"),
            size: std::mem::size_of::<Uniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let texture = create_video_texture(device, 1, 1);
        let group = create_group(device, &layout, &texture, &sampler, &uniform);
        Self { pipeline, layout, sampler, uniform, texture, group, size: (1, 1), target_is_srgb: target.is_srgb() }
    }

    fn prepare(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, img: Option<&RgbaFrame>, settings: ColorSettings) {
        if let Some(img) = img {
            if self.size != (img.width, img.height) {
                self.texture = create_video_texture(device, img.width, img.height);
                self.group = create_group(device, &self.layout, &self.texture, &self.sampler, &self.uniform);
                self.size = (img.width, img.height);
            }
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &self.texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                &img.rgba,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(4 * img.width),
                    rows_per_image: Some(img.height),
                },
                wgpu::Extent3d { width: img.width, height: img.height, depth_or_array_layers: 1 },
            );
        }
        queue.write_buffer(&self.uniform, 0, bytemuck::bytes_of(&settings.uniforms(self.target_is_srgb)));
    }

    fn paint(&self, pass: &mut wgpu::RenderPass<'static>) {
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.group, &[]);
        pass.draw(0..3, 0..1);
    }
}

/// The callback is recreated per frame and carries only that frame's upload.
pub struct VideoCallback {
    pub frame: Option<Arc<RgbaFrame>>,
    pub settings: ColorSettings,
}

impl CallbackTrait for VideoCallback {
    fn prepare(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        _screen: &egui_wgpu::ScreenDescriptor,
        _encoder: &mut wgpu::CommandEncoder,
        resources: &mut CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        if let Some(renderer) = resources.get_mut::<GpuVideo>() {
            renderer.prepare(device, queue, self.frame.as_deref(), self.settings);
        }
        Vec::new()
    }

    fn paint(
        &self,
        _info: eframe::egui::PaintCallbackInfo,
        pass: &mut wgpu::RenderPass<'static>,
        resources: &CallbackResources,
    ) {
        if let Some(renderer) = resources.get::<GpuVideo>() {
            renderer.paint(pass);
        }
    }
}
