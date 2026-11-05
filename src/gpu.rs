//! Custom wgpu paint callback embedded in egui. GPU fragment shader handles
//! ALL color adjustments; the CPU only converts MJPEG to RGBA and uploads it.
use std::{borrow::Cow, fs, io::{self, Write}, path::Path, sync::Arc};
use anyhow::{Context, Result};
use egui_wgpu::{wgpu, CallbackResources, CallbackTrait};
use crate::capture::RgbaFrame;

#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
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
    pub const EXPOSURE: std::ops::RangeInclusive<f32> = -2.0..=2.0;
    pub const GAMMA: std::ops::RangeInclusive<f32> = 0.60..=1.60;
    pub const HIGHLIGHTS: std::ops::RangeInclusive<f32> = 0.0..=1.0;
    pub const WHITE: std::ops::RangeInclusive<f32> = 0.55..=1.0;
    pub const SATURATION: std::ops::RangeInclusive<f32> = 0.0..=1.5;
    pub const SOFT_WHITE: Self = Self { exposure: 0.0, gamma: 1.0, highlights: 0.0, white: 0.84, saturation: 1.0 };

    fn validate(self) -> Result<()> {
        for (name, value, range) in [
            ("Exposure", self.exposure, Self::EXPOSURE),
            ("Gamma", self.gamma, Self::GAMMA),
            ("Highlights", self.highlights, Self::HIGHLIGHTS),
            ("White peak", self.white, Self::WHITE),
            ("Saturation", self.saturation, Self::SATURATION),
        ] {
            anyhow::ensure!(value.is_finite() && range.contains(&value), "{name} must be within {range:?}");
        }
        Ok(())
    }

    pub fn load(path: &Path) -> Result<Option<Self>> {
        let data = match fs::read(path) {
            Ok(data) => data,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error).context("Read GPU preset"),
        };
        let settings: Self = serde_json::from_slice(&data).context("Invalid GPU preset JSON")?;
        settings.validate()?;
        Ok(Some(settings))
    }

    pub fn save(self, path: &Path) -> Result<()> {
        self.validate()?;
        let data = serde_json::to_vec_pretty(&self)?;
        let parent = path.parent().context("GPU preset has no parent directory")?;
        fs::create_dir_all(parent).context("Create GPU preset directory")?;
        let temporary = path.with_extension(format!("json.{}.tmp", std::process::id()));
        let mut file = fs::OpenOptions::new().write(true).create_new(true)
            .open(&temporary).context("Create temporary GPU preset")?;
        // Replace only after the complete new preset is safely written.
        let result = (|| -> io::Result<()> {
            file.write_all(&data)?;
            file.sync_all()?;
            fs::rename(&temporary, path)
        })();
        if result.is_err() { let _ = fs::remove_file(&temporary); }
        result.context("Replace GPU preset")
    }

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_round_trip_reject_invalid_values_and_preserve_previous_file() {
        let root = std::env::temp_dir().join(format!("nanokvm-preset-{}-{}", std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        let path = root.join("colors.json");
        assert_eq!(ColorSettings::load(&path).unwrap(), None);
        for settings in [ColorSettings::default(), ColorSettings::SOFT_WHITE,
            ColorSettings { exposure: -2.0, gamma: 0.6, highlights: 0.0, white: 0.55, saturation: 0.0 },
            ColorSettings { exposure: 2.0, gamma: 1.6, highlights: 1.0, white: 1.0, saturation: 1.5 },
        ] {
            settings.save(&path).unwrap();
            assert_eq!(ColorSettings::load(&path).unwrap(), Some(settings));
        }
        let saved = fs::read(&path).unwrap();
        for (field, value) in [("exposure", -2.1), ("exposure", 2.1), ("gamma", 0.0), ("gamma", 1.7),
            ("highlights", -0.1), ("highlights", 1.1), ("white", 0.5), ("white", 1.1),
            ("saturation", -0.1), ("saturation", 1.6)] {
            let mut json = serde_json::to_value(ColorSettings::default()).unwrap();
            json[field] = serde_json::json!(value);
            fs::write(&path, serde_json::to_vec(&json).unwrap()).unwrap();
            assert!(ColorSettings::load(&path).is_err(), "{field}={value}");
            fs::write(&path, &saved).unwrap();
            let settings: ColorSettings = serde_json::from_value(json).unwrap();
            assert!(settings.save(&path).is_err());
            assert_eq!(fs::read(&path).unwrap(), saved);
        }
        for gamma in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            assert!(ColorSettings { gamma, ..Default::default() }.save(&path).is_err());
            assert_eq!(fs::read(&path).unwrap(), saved);
        }
        for bytes in [b"broken JSON".as_slice(), b"{}", b"{\"gamma\":null}"] {
            fs::write(&path, bytes).unwrap();
            assert!(ColorSettings::load(&path).is_err());
        }
        fs::write(&path, &saved).unwrap();
        let temporary = path.with_extension(format!("json.{}.tmp", std::process::id()));
        fs::write(&temporary, b"another writer").unwrap();
        assert!(ColorSettings::default().save(&path).is_err());
        assert_eq!(fs::read(&path).unwrap(), saved);
        assert_eq!(fs::read(&temporary).unwrap(), b"another writer");
        fs::remove_file(&temporary).unwrap();
        let directory = root.join("directory.json");
        fs::create_dir(&directory).unwrap();
        assert!(ColorSettings::load(&directory).is_err());
        assert!(ColorSettings::default().save(&directory).is_err());
        assert!(directory.is_dir());
        assert!(!directory.with_extension(format!("json.{}.tmp", std::process::id())).exists());
        assert!(ColorSettings::default().save(&path.join("child.json")).is_err());
        assert_eq!(fs::read(&path).unwrap(), saved);
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        if fs::metadata(&root).unwrap().uid() != 0 {
            fs::set_permissions(&root, fs::Permissions::from_mode(0o500)).unwrap();
            let denied = ColorSettings::default().save(&path);
            fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
            assert!(denied.is_err());
            assert_eq!(fs::read(&path).unwrap(), saved);
            fs::set_permissions(&path, fs::Permissions::from_mode(0)).unwrap();
            let unreadable = ColorSettings::load(&path);
            fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
            assert!(unreadable.is_err());
        }
        fs::remove_dir_all(root).unwrap();
    }

    fn block_on<F: std::future::Future>(future: F) -> F::Output {
        struct WakeThread(std::thread::Thread);
        impl std::task::Wake for WakeThread {
            fn wake(self: Arc<Self>) { self.0.unpark(); }
        }
        let waker = std::task::Waker::from(Arc::new(WakeThread(std::thread::current())));
        let mut context = std::task::Context::from_waker(&waker);
        let mut future = std::pin::pin!(future);
        loop {
            match future.as_mut().poll(&mut context) {
                std::task::Poll::Ready(value) => return value,
                std::task::Poll::Pending => std::thread::park(),
            }
        }
    }

    fn render(renderer: &GpuVideo, device: &wgpu::Device, queue: &wgpu::Queue, format: wgpu::TextureFormat) -> Vec<u8> {
        let target = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("T-11 render target"),
            size: wgpu::Extent3d { width: 4, height: 2, depth_or_array_layers: 1 },
            mip_level_count: 1, sample_count: 1, dimension: wgpu::TextureDimension::D2,
            format, usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC, view_formats: &[],
        });
        let view = target.create_view(&Default::default());
        let readback = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("T-11 pixel readback"), size: 512,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false,
        });
        let mut encoder = device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("T-11 production shader"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view, resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT), store: wgpu::StoreOp::Store },
                })],
                ..Default::default()
            }).forget_lifetime();
            renderer.paint(&mut pass);
        }
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo { texture: &target, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            wgpu::TexelCopyBufferInfo { buffer: &readback, layout: wgpu::TexelCopyBufferLayout {
                offset: 0, bytes_per_row: Some(256), rows_per_image: Some(2),
            } },
            wgpu::Extent3d { width: 4, height: 2, depth_or_array_layers: 1 },
        );
        queue.submit([encoder.finish()]);
        let (tx, rx) = std::sync::mpsc::channel();
        readback.slice(..).map_async(wgpu::MapMode::Read, move |result| tx.send(result).unwrap());
        device.poll(wgpu::Maintain::Wait);
        rx.recv().unwrap().unwrap();
        let mapped = readback.slice(..).get_mapped_range();
        [mapped[..16].to_vec(), mapped[256..272].to_vec()].concat()
    }

    #[test]
    #[ignore = "requires a GPU adapter; run explicitly with --ignored --nocapture"]
    fn gpu_render_controls_and_reset() {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::VULKAN, ..Default::default()
        });
        let adapter = block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
            .expect("T-11 needs a Vulkan adapter");
        eprintln!("T-11 adapter: {:?}", adapter.get_info());
        let (device, queue) = block_on(adapter.request_device(&wgpu::DeviceDescriptor::default(), None)).unwrap();
        let frame = RgbaFrame { width: 4, height: 2, rgba: vec![
            0,0,0,255, 128,128,128,255, 255,255,255,255, 200,80,40,255,
            255,0,0,255, 0,255,0,255, 0,0,255,255, 40,90,170,255,
        ] };
        let close = |actual: u8, expected: u8| assert!(actual.abs_diff(expected) <= 2, "{actual} != {expected}");
        for format in [wgpu::TextureFormat::Rgba8UnormSrgb, wgpu::TextureFormat::Rgba8Unorm] {
            let mut renderer = GpuVideo::new(&device, format);
            renderer.prepare(&device, &queue, Some(&frame), ColorSettings::default());
            let output = render(&renderer, &device, &queue, format);
            for (&actual, &expected) in output.iter().zip(&frame.rgba) { close(actual, expected); }
            for (settings, gray, white) in [
                (ColorSettings { exposure: 1.0, ..Default::default() }, 176, 255),
                (ColorSettings { gamma: 1.6, ..Default::default() }, 83, 255),
                (ColorSettings { highlights: 1.0, ..Default::default() }, 117, 188),
                (ColorSettings { white: 0.55, ..Default::default() }, 67, 140),
                (ColorSettings::SOFT_WHITE, 106, 214),
                (ColorSettings { saturation: 0.0, ..Default::default() }, 128, 255),
            ] {
                // Controls must update the retained texture even without another captured frame.
                renderer.prepare(&device, &queue, None, settings);
                let output = render(&renderer, &device, &queue, format);
                for channel in 0..3 { close(output[4 + channel], gray); close(output[8 + channel], white); }
                assert_eq!(&output[..4], &[0, 0, 0, 255]);
                assert!(output.chunks_exact(4).all(|pixel| pixel[3] == 255));
                if settings.saturation == 0.0 {
                    for channel in &output[12..15] { close(*channel, 118); }
                    assert!(output.chunks_exact(4).all(|pixel| pixel[0] == pixel[1] && pixel[1] == pixel[2]));
                }
            }
            renderer.prepare(&device, &queue, Some(&RgbaFrame { width: 1, height: 1, rgba: vec![128,128,128,255] }), ColorSettings::default());
            let output = render(&renderer, &device, &queue, format);
            for pixel in output.chunks_exact(4) { for channel in &pixel[..3] { close(*channel, 128); } }
            renderer.prepare(&device, &queue, Some(&frame), ColorSettings::default());
            let reset = render(&renderer, &device, &queue, format);
            for (&actual, &expected) in reset.iter().zip(&frame.rgba) { close(actual, expected); }
        }
    }
}
