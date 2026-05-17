//! Desktop GL backend: winit 0.30 + glutin 0.32 + glow 0.14.
//!
//! One window, one GL context, one texture, one fullscreen-quad shader.
//! No FBOs, no post-FX, no softbuffer status window.

use std::num::NonZeroU32;

use glutin::config::ConfigTemplateBuilder;
use glutin::context::{ContextApi, ContextAttributesBuilder, Version};
use glutin::display::GetGlDisplay;
use glutin::prelude::*;
use glutin::surface::{Surface, SurfaceAttributesBuilder, WindowSurface};
use glutin_winit::DisplayBuilder;
use glow::HasContext;
use winit::dpi::PhysicalSize;
use winit::event::{Event, WindowEvent};
use winit::event_loop::{ControlFlow, EventLoop};
use winit::raw_window_handle::HasWindowHandle;
use winit::window::Window;

use super::shader;
use super::shader::QUAD;
use super::text::TextOverlay;

pub struct WinitGlTarget {
    gl: glow::Context,
    surface: Surface<WindowSurface>,
    gl_context: glutin::context::PossiblyCurrentContext,
    _window: Window,
    event_loop: EventLoop<()>,
    should_close: bool,

    // GL objects
    program: <glow::Context as HasContext>::Program,
    vbo: <glow::Context as HasContext>::Buffer,
    texture: <glow::Context as HasContext>::Texture,
    u_alpha: Option<<glow::Context as HasContext>::UniformLocation>,
    u_tex: Option<<glow::Context as HasContext>::UniformLocation>,

    // Track texture dimensions to decide tex_image_2d vs tex_sub_image_2d
    last_tex_w: u32,
    last_tex_h: u32,

    // Cached vertex attribute locations (set once at program compile time)
    pos_loc: u32,
    uv_loc: u32,

    // Text overlay (menus rendered on top of the video layer)
    text: TextOverlay,

    pipeline: crate::render::shader_pipeline::ShaderPipeline,
    start_time: std::time::Instant,
}

impl WinitGlTarget {
    pub fn new(width: u32, height: u32, title: &str, profile: crate::render::shader_assembly::GlesProfile) -> anyhow::Result<Self> {
        let event_loop = EventLoop::new()
            .map_err(|e| anyhow::anyhow!("event loop: {e}"))?;

        let window_attributes = Window::default_attributes()
            .with_inner_size(PhysicalSize::new(width, height))
            .with_title(title);

        let template = ConfigTemplateBuilder::new().with_alpha_size(8);

        let display_builder =
            DisplayBuilder::new().with_window_attributes(Some(window_attributes));

        let (window, gl_config) = display_builder
            .build(&event_loop, template, |mut configs| configs.next().unwrap())
            .map_err(|e| anyhow::anyhow!("display build: {e}"))?;

        let window = window.ok_or_else(|| anyhow::anyhow!("no window from glutin"))?;

        let raw = window
            .window_handle()
            .map_err(|e| anyhow::anyhow!("window handle: {e}"))?
            .as_raw();

        let context_attrs = ContextAttributesBuilder::new()
            .with_context_api(ContextApi::OpenGl(Some(Version::new(3, 0))))
            .build(Some(raw));

        let not_current = unsafe {
            gl_config
                .display()
                .create_context(&gl_config, &context_attrs)
                .map_err(|e| anyhow::anyhow!("create context: {e}"))?
        };

        let surface_attrs = SurfaceAttributesBuilder::<WindowSurface>::new().build(
            raw,
            NonZeroU32::new(width).unwrap(),
            NonZeroU32::new(height).unwrap(),
        );

        let surface = unsafe {
            gl_config
                .display()
                .create_window_surface(&gl_config, &surface_attrs)
                .map_err(|e| anyhow::anyhow!("surface: {e}"))?
        };

        let gl_context = not_current
            .make_current(&surface)
            .map_err(|e| anyhow::anyhow!("make current: {e}"))?;

        let gl = unsafe {
            glow::Context::from_loader_function_cstr(|s| {
                gl_config.display().get_proc_address(s) as *const _
            })
        };

        // Compile shader program
        let program = unsafe { shader::compile_program(&gl, shader::VERT, shader::FRAG)? };

        // Build interleaved VBO: (x,y,u,v) × 6 vertices
        let vbo = unsafe {
            let vbo = gl
                .create_buffer()
                .map_err(|e| anyhow::anyhow!("create vbo: {e}"))?;
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(vbo));
            let bytes: &[u8] = bytemuck::cast_slice(QUAD);
            gl.buffer_data_u8_slice(glow::ARRAY_BUFFER, bytes, glow::STATIC_DRAW);
            vbo
        };

        // Allocate texture (w × h, RGBA) — content filled on first draw_video_layer
        let texture = unsafe {
            let tex = gl
                .create_texture()
                .map_err(|e| anyhow::anyhow!("create texture: {e}"))?;
            gl.bind_texture(glow::TEXTURE_2D, Some(tex));
            gl.tex_image_2d(
                glow::TEXTURE_2D,
                0,
                glow::RGBA as i32,
                width as i32,
                height as i32,
                0,
                glow::RGBA,
                glow::UNSIGNED_BYTE,
                None,
            );
            gl.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_MIN_FILTER,
                glow::LINEAR as i32,
            );
            gl.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_MAG_FILTER,
                glow::LINEAR as i32,
            );
            gl.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_WRAP_S,
                glow::CLAMP_TO_EDGE as i32,
            );
            gl.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_WRAP_T,
                glow::CLAMP_TO_EDGE as i32,
            );
            tex
        };

        // Cache uniform locations
        let u_alpha = unsafe { gl.get_uniform_location(program, "u_alpha") };
        let u_tex = unsafe { gl.get_uniform_location(program, "u_tex") };

        // Cache vertex attribute locations (constant after program compile)
        let pos_loc = unsafe {
            gl.get_attrib_location(program, "a_pos").expect("a_pos not found in shader") as u32
        };
        let uv_loc = unsafe {
            gl.get_attrib_location(program, "a_uv").expect("a_uv not found in shader") as u32
        };

        // Build text overlay (atlas texture + dynamic VBO + shader)
        let text = unsafe { TextOverlay::new(&gl)? };

        let shaders_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("shaders");
        let min_gles = match profile {
            crate::render::shader_assembly::GlesProfile::V100 => crate::shader::GlesVersion::V100,
            crate::render::shader_assembly::GlesProfile::V310 => crate::shader::GlesVersion::V310,
        };
        let library = crate::shader::ShaderLibrary::load_dir_for_profile(&shaders_dir, min_gles)?;
        let mut pipeline = crate::render::shader_pipeline::ShaderPipeline::new(profile, library);
        // SAFETY: GL context made current above (gl_context is .make_current()-ed).
        unsafe {
            pipeline.select(&gl, "passthrough")?;
        }
        let start_time = std::time::Instant::now();

        Ok(Self {
            gl,
            surface,
            gl_context,
            _window: window,
            event_loop,
            should_close: false,
            program,
            vbo,
            texture,
            u_alpha,
            u_tex,
            last_tex_w: width,
            last_tex_h: height,
            pos_loc,
            uv_loc,
            text,
            pipeline,
            start_time,
        })
    }

    /// Non-blocking event drain. Returns any KeyEvents that occurred since last pump.
    pub fn pump(&mut self) -> Vec<winit::event::KeyEvent> {
        use winit::platform::pump_events::EventLoopExtPumpEvents;

        let timeout = Some(std::time::Duration::ZERO);
        let mut key_events: Vec<winit::event::KeyEvent> = Vec::new();
        let mut should_close = self.should_close;
        let mut new_size: Option<(u32, u32)> = None;

        #[allow(deprecated)]
        self.event_loop.pump_events(timeout, |event, target| {
            target.set_control_flow(ControlFlow::Poll);
            if let Event::WindowEvent { event, .. } = event {
                match event {
                    WindowEvent::CloseRequested => {
                        should_close = true;
                    }
                    WindowEvent::Resized(size) => {
                        new_size = Some((size.width, size.height));
                    }
                    WindowEvent::KeyboardInput { event, .. } => {
                        key_events.push(event);
                    }
                    _ => {}
                }
            }
        });

        self.should_close = should_close;

        if let Some((w, h)) = new_size {
            if let (Some(nz_w), Some(nz_h)) = (NonZeroU32::new(w), NonZeroU32::new(h)) {
                self.surface.resize(&self.gl_context, nz_w, nz_h);
                unsafe {
                    self.gl.viewport(0, 0, w as i32, h as i32);
                }
            }
        }

        key_events
    }

    pub fn should_close(&self) -> bool {
        self.should_close
    }

    pub fn begin_frame(&mut self) {
        unsafe {
            self.gl.clear_color(0.0, 0.0, 0.0, 1.0);
            self.gl.clear(glow::COLOR_BUFFER_BIT);
        }
    }

    /// Upload `rgba` as a GL texture and draw it to a fullscreen quad.
    /// `rgba` must be exactly `w * h * 4` bytes.
    pub fn draw_video_layer(&mut self, rgba: &[u8], w: u32, h: u32, alpha: f32) {
        unsafe {
            let gl = &self.gl;

            gl.bind_texture(glow::TEXTURE_2D, Some(self.texture));

            if w != self.last_tex_w || h != self.last_tex_h {
                // Re-allocate texture storage for the new dimensions.
                gl.tex_image_2d(
                    glow::TEXTURE_2D,
                    0,
                    glow::RGBA as i32,
                    w as i32,
                    h as i32,
                    0,
                    glow::RGBA,
                    glow::UNSIGNED_BYTE,
                    None,
                );
                self.last_tex_w = w;
                self.last_tex_h = h;
            }

            // Upload pixel data
            gl.tex_sub_image_2d(
                glow::TEXTURE_2D,
                0,
                0,
                0,
                w as i32,
                h as i32,
                glow::RGBA,
                glow::UNSIGNED_BYTE,
                glow::PixelUnpackData::Slice(rgba),
            );

            // Determine current viewport size for the shader pass.
            let mut vp = [0i32; 4];
            gl.get_parameter_i32_slice(glow::VIEWPORT, &mut vp);
            let (sw, sh) = (vp[2].max(1) as u32, vp[3].max(1) as u32);
            let t = self.start_time.elapsed().as_secs_f32();

            // Run the active shader over the freshly uploaded video frame.
            let shaded = self
                .pipeline
                .apply(gl, self.texture, w, h, sw, sh, t)
                .unwrap_or_else(|e| {
                    tracing::warn!("shader apply failed, falling back to source texture: {e}");
                    self.texture
                });

            gl.use_program(Some(self.program));

            // Bind texture to unit 0
            gl.active_texture(glow::TEXTURE0);
            gl.bind_texture(glow::TEXTURE_2D, Some(shaded));
            gl.uniform_1_i32(self.u_tex.as_ref(), 0);

            // Set alpha
            gl.uniform_1_f32(self.u_alpha.as_ref(), alpha);

            // Bind VBO and set vertex attributes
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(self.vbo));

            // a_pos: 2 floats, stride 16 bytes, offset 0
            gl.enable_vertex_attrib_array(self.pos_loc);
            gl.vertex_attrib_pointer_f32(self.pos_loc, 2, glow::FLOAT, false, 16, 0);

            // a_uv: 2 floats, stride 16 bytes, offset 8
            gl.enable_vertex_attrib_array(self.uv_loc);
            gl.vertex_attrib_pointer_f32(self.uv_loc, 2, glow::FLOAT, false, 16, 8);

            gl.enable(glow::BLEND);
            gl.blend_func(glow::SRC_ALPHA, glow::ONE_MINUS_SRC_ALPHA);
            gl.draw_arrays(glow::TRIANGLES, 0, 6);
            gl.disable(glow::BLEND);

            // Disable attrib arrays to leave clean GL state
            gl.disable_vertex_attrib_array(self.pos_loc);
            gl.disable_vertex_attrib_array(self.uv_loc);
        }
    }

    /// Alpha-blend `rgba` over the previously drawn video layer at `mix` opacity.
    /// Reuses `draw_video_layer` with `alpha = mix`; early-returns when mix ≤ 0.
    pub fn draw_detour_layer(&mut self, rgba: &[u8], w: u32, h: u32, mix: f32) {
        if mix <= 0.0 {
            return;
        }
        self.draw_video_layer(rgba, w, h, mix.clamp(0.0, 1.0));
    }

    /// Draw the menu/status text grid as a full-window overlay on top of the
    /// video layer. The grid's background is rendered semi-transparent so the
    /// video bleeds through; glyph pixels themselves are fully opaque.
    pub fn draw_text_grid(&mut self, grid: &crate::status::grid::TextGrid) {
        unsafe {
            self.text.draw(&self.gl, grid, 0.55);
        }
    }

    pub fn end_frame(&mut self) {
        if let Err(e) = self.surface.swap_buffers(&self.gl_context) {
            tracing::warn!("swap_buffers: {e}");
        }
    }

    pub fn select_shader(&mut self, name: &str, params: [f32; 8]) -> anyhow::Result<()> {
        unsafe {
            self.pipeline.select(&self.gl, name)
                .map_err(|e| anyhow::anyhow!("select_shader {name}: {e}"))?;
        }
        self.pipeline.set_params(params);
        Ok(())
    }

    pub fn clear_shader(&mut self) {
        self.pipeline.clear_active();
    }

    pub fn pulse_shader_trigger(&mut self) {
        self.pipeline.pulse_trigger();
    }

    pub fn invalidate_shader(&mut self, name: &str) {
        self.pipeline.invalidate(name);
    }

    pub fn upsert_shader(&mut self, name: &str, shader: crate::shader::LoadedShader) {
        self.pipeline.library_mut().upsert(name, shader);
    }

    pub fn set_shader_params(&mut self, params: [f32; 8]) {
        self.pipeline.set_params(params);
    }
}

