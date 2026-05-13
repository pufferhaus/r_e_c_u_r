#![cfg(all(feature = "pi", target_os = "linux"))]

//! Raspberry Pi composite-output backend via DRM/KMS + GBM + EGL.
//!
//! Tested reference: mandleROT/src/render/pi.rs on Pi 3 B+ with
//! `enable_tvout=1` and `sdtv_mode=0` in `/boot/firmware/config.txt`.
//! Same code path on Pi 4/5 with appropriate composite hardware adapter.
//!
//! This module exposes a `PiTarget` with the same method surface as
//! `WinitGlTarget` in `desktop.rs`: `new`, `pump`, `should_close`,
//! `begin_frame`, `draw_video_layer`, `end_frame`.
//!
//! Runtime testing on real Pi hardware is pending hardware access.
//! The cross-build (`cross build --no-default-features --features pi`) is
//! the verification gate for this task.

use std::os::fd::AsFd;

use drm::control::{connector, framebuffer, Device as ControlDevice};
use drm::Device as BasicDevice;
use gbm::{AsRaw, BufferObjectFlags, Device as GbmDevice, Format as GbmFormat, Surface as GbmSurface};
use glow::HasContext;

use crate::error::{Error, Result};
use super::shader;

// ── DRM card wrapper ──────────────────────────────────────────────────────────

struct PiCard {
    file: std::fs::File,
}

impl AsFd for PiCard {
    fn as_fd(&self) -> std::os::fd::BorrowedFd<'_> {
        self.file.as_fd()
    }
}

impl BasicDevice for PiCard {}
impl ControlDevice for PiCard {}

impl PiCard {
    fn open(path: &str) -> Result<Self> {
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(path)
            .map_err(|e| Error::Other(format!("open {path}: {e}")))?;
        Ok(Self { file })
    }

    fn open_default() -> Result<Self> {
        for path in ["/dev/dri/card0", "/dev/dri/card1"] {
            if let Ok(c) = Self::open(path) {
                return Ok(c);
            }
        }
        Err(Error::Other("no DRM device found".into()))
    }

    /// Find the first connected composite (TV) connector.
    fn find_composite_connector(&self) -> Result<connector::Info> {
        let resources = self
            .resource_handles()
            .map_err(|e| Error::Other(format!("drm resources: {e}")))?;
        for handle in resources.connectors() {
            let info = self
                .get_connector(*handle, false)
                .map_err(|e| Error::Other(format!("drm connector: {e}")))?;
            // Composite/TV/SVideo are analog with no hot-plug detect — KMS state
            // is always Unknown. Accept Unknown; reject explicit Disconnected.
            if info.state() == connector::State::Disconnected {
                continue;
            }
            use connector::Interface::*;
            if matches!(info.interface(), Composite | TV | SVideo) {
                return Ok(info);
            }
        }
        Err(Error::Other("no connected composite/TV connector found".into()))
    }
}

// ── GBM framebuffer adapter ───────────────────────────────────────────────────

/// Adapts `gbm::BufferObject<()>` to `drm::buffer::PlanarBuffer` without
/// advertising a modifier. Returning `None` for `modifier()` lets us pass
/// `FbCmd2Flags::empty()` to `add_planar_framebuffer`, avoiding the modifier
/// assertion on plain ARGB8888 SCANOUT buffers.
struct GbmFb<'a> {
    bo: &'a gbm::BufferObject<()>,
}

impl<'a> drm::buffer::PlanarBuffer for GbmFb<'a> {
    fn size(&self) -> (u32, u32) {
        (self.bo.width().unwrap_or(0), self.bo.height().unwrap_or(0))
    }
    fn format(&self) -> drm::buffer::DrmFourcc {
        drm::buffer::DrmFourcc::Argb8888
    }
    fn modifier(&self) -> Option<drm::buffer::DrmModifier> {
        None
    }
    fn pitches(&self) -> [u32; 4] {
        [self.bo.stride().unwrap_or(0), 0, 0, 0]
    }
    fn handles(&self) -> [Option<drm::buffer::Handle>; 4] {
        [
            Some(<gbm::BufferObject<()> as drm::buffer::Buffer>::handle(self.bo)),
            None,
            None,
            None,
        ]
    }
    fn offsets(&self) -> [u32; 4] {
        [0; 4]
    }
}

// ── EGL + GBM context ────────────────────────────────────────────────────────

struct PiContext {
    card: PiCard,
    #[allow(dead_code)]
    gbm: GbmDevice<PiCard>,
    surface: GbmSurface<()>,
    egl: khronos_egl::DynamicInstance<khronos_egl::EGL1_5>,
    egl_display: khronos_egl::Display,
    #[allow(dead_code)]
    egl_context: khronos_egl::Context,
    egl_surface: khronos_egl::Surface,
    crtc_handle: drm::control::crtc::Handle,
    connector_handle: drm::control::connector::Handle,
    mode: drm::control::Mode,
    #[allow(dead_code)]
    width: u32,
    #[allow(dead_code)]
    height: u32,
    gl: glow::Context,
}

impl PiContext {
    fn create(width_hint: u32, height_hint: u32) -> Result<Self> {
        let card = PiCard::open_default()?;
        let conn = card.find_composite_connector()?;
        let mode = conn
            .modes()
            .iter()
            .find(|m| m.size() == (width_hint as u16, height_hint as u16))
            .or_else(|| conn.modes().first())
            .copied()
            .ok_or_else(|| Error::Other("no display modes available".into()))?;
        let (width, height) = (mode.size().0 as u32, mode.size().1 as u32);

        let encoder_handle = conn
            .current_encoder()
            .ok_or_else(|| Error::Other("connector has no encoder".into()))?;
        let enc = card
            .get_encoder(encoder_handle)
            .map_err(|e| Error::Other(format!("drm encoder: {e}")))?;
        let crtc_handle = enc
            .crtc()
            .ok_or_else(|| Error::Other("encoder has no CRTC".into()))?;

        let card_for_gbm = PiCard {
            file: card
                .file
                .try_clone()
                .map_err(|e| Error::Other(format!("dup fd: {e}")))?,
        };
        let gbm = GbmDevice::new(card_for_gbm)
            .map_err(|e| Error::Other(format!("gbm device: {e}")))?;
        let surface = gbm
            .create_surface::<()>(
                width,
                height,
                GbmFormat::Argb8888,
                BufferObjectFlags::SCANOUT | BufferObjectFlags::RENDERING,
            )
            .map_err(|e| Error::Other(format!("gbm surface: {e}")))?;

        let egl = unsafe {
            khronos_egl::DynamicInstance::<khronos_egl::EGL1_5>::load_required()
                .map_err(|e| Error::Other(format!("load EGL: {e}")))?
        };
        let egl_display = unsafe {
            egl.get_display(gbm.as_raw_mut() as *mut _)
                .ok_or_else(|| Error::Other("get EGL display failed".into()))?
        };
        egl.initialize(egl_display)
            .map_err(|e| Error::Other(format!("egl init: {e}")))?;

        let attribs = [
            khronos_egl::SURFACE_TYPE,
            khronos_egl::WINDOW_BIT,
            khronos_egl::RED_SIZE,   8,
            khronos_egl::GREEN_SIZE, 8,
            khronos_egl::BLUE_SIZE,  8,
            khronos_egl::ALPHA_SIZE, 8,
            khronos_egl::RENDERABLE_TYPE,
            khronos_egl::OPENGL_ES2_BIT,
            khronos_egl::NONE,
        ];
        let config = egl
            .choose_first_config(egl_display, &attribs)
            .map_err(|e| Error::Other(format!("egl choose config: {e}")))?
            .ok_or_else(|| Error::Other("no matching EGL config".into()))?;

        let ctx_attribs = [khronos_egl::CONTEXT_CLIENT_VERSION, 2, khronos_egl::NONE];
        egl.bind_api(khronos_egl::OPENGL_ES_API)
            .map_err(|e| Error::Other(format!("egl bind api: {e}")))?;
        let egl_context = egl
            .create_context(egl_display, config, None, &ctx_attribs)
            .map_err(|e| Error::Other(format!("egl create context: {e}")))?;
        let egl_surface = unsafe {
            egl.create_window_surface(
                egl_display,
                config,
                surface.as_raw_mut() as *mut _,
                None,
            )
            .map_err(|e| Error::Other(format!("egl create surface: {e}")))?
        };
        egl.make_current(
            egl_display,
            Some(egl_surface),
            Some(egl_surface),
            Some(egl_context),
        )
        .map_err(|e| Error::Other(format!("egl make current: {e}")))?;

        let gl = unsafe {
            glow::Context::from_loader_function_cstr(|s| {
                egl.get_proc_address(s.to_str().unwrap_or(""))
                    .map(|f| f as *const _)
                    .unwrap_or(std::ptr::null())
            })
        };

        let version = unsafe { gl.get_parameter_string(glow::VERSION) };
        let renderer = unsafe { gl.get_parameter_string(glow::RENDERER) };
        tracing::info!(gl_version = %version, gl_renderer = %renderer, "Pi GL context");
        if version.is_empty() {
            return Err(Error::Other("empty GL_VERSION from EGL".into()));
        }

        Ok(Self {
            card,
            gbm,
            surface,
            egl,
            egl_display,
            egl_context,
            egl_surface,
            crtc_handle,
            connector_handle: conn.handle(),
            mode,
            width,
            height,
            gl,
        })
    }
}

// ── PiTarget — public API ─────────────────────────────────────────────────────

pub struct PiTarget {
    ctx: PiContext,

    // GL objects for the fullscreen-quad video layer
    program: <glow::Context as HasContext>::Program,
    vbo: <glow::Context as HasContext>::Buffer,
    texture: <glow::Context as HasContext>::Texture,
    u_alpha: Option<<glow::Context as HasContext>::UniformLocation>,
    u_tex: Option<<glow::Context as HasContext>::UniformLocation>,
    last_tex_w: u32,
    last_tex_h: u32,

    // Cached vertex attribute locations (set once at program compile time)
    pos_loc: u32,
    uv_loc: u32,

    // KMS framebuffer tracking (page-flip double buffer)
    scanning: Option<(framebuffer::Handle, gbm::BufferObject<()>)>,

    should_exit: bool,
}

impl PiTarget {
    pub fn new(w: u32, h: u32, _title: &str) -> anyhow::Result<Self> {
        let ctx = PiContext::create(w, h)?;

        let program = unsafe { shader::compile_program(&ctx.gl, shader::VERT, shader::FRAG)? };

        let vbo = unsafe {
            let vbo = ctx.gl
                .create_buffer()
                .map_err(|e| anyhow::anyhow!("create vbo: {e}"))?;
            ctx.gl.bind_buffer(glow::ARRAY_BUFFER, Some(vbo));
            let bytes: &[u8] = bytemuck::cast_slice(shader::QUAD);
            ctx.gl.buffer_data_u8_slice(glow::ARRAY_BUFFER, bytes, glow::STATIC_DRAW);
            vbo
        };

        let texture = unsafe {
            let tex = ctx.gl
                .create_texture()
                .map_err(|e| anyhow::anyhow!("create texture: {e}"))?;
            ctx.gl.bind_texture(glow::TEXTURE_2D, Some(tex));
            ctx.gl.tex_image_2d(
                glow::TEXTURE_2D, 0, glow::RGBA as i32,
                w as i32, h as i32, 0,
                glow::RGBA, glow::UNSIGNED_BYTE, None,
            );
            ctx.gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MIN_FILTER, glow::LINEAR as i32);
            ctx.gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MAG_FILTER, glow::LINEAR as i32);
            ctx.gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_WRAP_S, glow::CLAMP_TO_EDGE as i32);
            ctx.gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_WRAP_T, glow::CLAMP_TO_EDGE as i32);
            tex
        };

        let u_alpha = unsafe { ctx.gl.get_uniform_location(program, "u_alpha") };
        let u_tex = unsafe { ctx.gl.get_uniform_location(program, "u_tex") };

        // Cache vertex attribute locations (constant after program compile)
        let pos_loc = unsafe {
            ctx.gl.get_attrib_location(program, "a_pos").expect("a_pos not found in shader") as u32
        };
        let uv_loc = unsafe {
            ctx.gl.get_attrib_location(program, "a_uv").expect("a_uv not found in shader") as u32
        };

        Ok(Self {
            ctx,
            program,
            vbo,
            texture,
            u_alpha,
            u_tex,
            last_tex_w: w,
            last_tex_h: h,
            pos_loc,
            uv_loc,
            scanning: None,
            should_exit: false,
        })
    }

    /// No event loop on Pi — evdev input is Phase 5. Returns empty vec.
    pub fn pump(&mut self) -> Vec<()> {
        Vec::new()
    }

    /// Always false until Phase 5 wires evdev SIGINT/SIGTERM handling.
    pub fn should_close(&self) -> bool {
        self.should_exit
    }

    pub fn begin_frame(&mut self) {
        unsafe {
            self.ctx.gl.clear_color(0.0, 0.0, 0.0, 1.0);
            self.ctx.gl.clear(glow::COLOR_BUFFER_BIT);
        }
    }

    /// Upload `rgba` as a GL texture and draw it to a fullscreen quad.
    /// `rgba` must be exactly `w * h * 4` bytes.
    pub fn draw_video_layer(&mut self, rgba: &[u8], w: u32, h: u32, alpha: f32) {
        unsafe {
            let gl = &self.ctx.gl;

            gl.bind_texture(glow::TEXTURE_2D, Some(self.texture));

            if w != self.last_tex_w || h != self.last_tex_h {
                gl.tex_image_2d(
                    glow::TEXTURE_2D, 0, glow::RGBA as i32,
                    w as i32, h as i32, 0,
                    glow::RGBA, glow::UNSIGNED_BYTE, None,
                );
                self.last_tex_w = w;
                self.last_tex_h = h;
            }

            gl.tex_sub_image_2d(
                glow::TEXTURE_2D, 0, 0, 0,
                w as i32, h as i32,
                glow::RGBA, glow::UNSIGNED_BYTE,
                glow::PixelUnpackData::Slice(rgba),
            );

            gl.use_program(Some(self.program));

            gl.active_texture(glow::TEXTURE0);
            gl.bind_texture(glow::TEXTURE_2D, Some(self.texture));
            gl.uniform_1_i32(self.u_tex.as_ref(), 0);
            gl.uniform_1_f32(self.u_alpha.as_ref(), alpha);

            gl.bind_buffer(glow::ARRAY_BUFFER, Some(self.vbo));

            // a_pos: 2 floats, stride 16 bytes, offset 0
            gl.enable_vertex_attrib_array(self.pos_loc);
            gl.vertex_attrib_pointer_f32(self.pos_loc, 2, glow::FLOAT, false, 16, 0);

            // a_uv: 2 floats, stride 16 bytes, offset 8
            gl.enable_vertex_attrib_array(self.uv_loc);
            gl.vertex_attrib_pointer_f32(self.uv_loc, 2, glow::FLOAT, false, 16, 8);

            gl.draw_arrays(glow::TRIANGLES, 0, 6);

            gl.disable_vertex_attrib_array(self.pos_loc);
            gl.disable_vertex_attrib_array(self.uv_loc);
        }
    }

    /// Swap EGL buffers then page-flip via `set_crtc` (blocking until vblank).
    ///
    /// `set_crtc` is used rather than async `page_flip` because composite
    /// output on the Pi 3B+ VEC encoder fires pageflip events unreliably —
    /// the kernel can silently detach the primary plane if the FB is freed
    /// while it's still scanning out. `set_crtc` blocks until the next vblank
    /// and handles plane attachment atomically.
    pub fn end_frame(&mut self) {
        // Swap EGL — this makes the GBM front buffer available.
        if let Err(e) = self.ctx.egl.swap_buffers(self.ctx.egl_display, self.ctx.egl_surface) {
            tracing::warn!("egl swap_buffers: {e}");
            return;
        }

        // Lock the front GBM buffer object.
        let bo = match unsafe { self.ctx.surface.lock_front_buffer() } {
            Ok(b) => b,
            Err(e) => {
                tracing::warn!("gbm lock_front_buffer: {e}");
                return;
            }
        };

        // Register it as a DRM framebuffer.
        let fb = match self.ctx.card.add_planar_framebuffer(
            &GbmFb { bo: &bo },
            drm::control::FbCmd2Flags::empty(),
        ) {
            Ok(fb) => fb,
            Err(e) => {
                tracing::warn!("add_planar_framebuffer: {e}");
                return;
            }
        };

        // Blocking mode-set / page-flip.
        if let Err(e) = self.ctx.card.set_crtc(
            self.ctx.crtc_handle,
            Some(fb),
            (0, 0),
            &[self.ctx.connector_handle],
            Some(self.ctx.mode),
        ) {
            tracing::warn!("set_crtc: {e}");
        }

        // Free the previously scanning framebuffer; keep the new one.
        if let Some((old_fb, _)) = self.scanning.replace((fb, bo)) {
            let _ = self.ctx.card.destroy_framebuffer(old_fb);
        }
    }
}

impl Drop for PiTarget {
    fn drop(&mut self) {
        if let Some((fb, _)) = self.scanning.take() {
            let _ = self.ctx.card.destroy_framebuffer(fb);
        }
    }
}

#[cfg(test)]
mod tests {
    // Pi-only hardware tests — see deploy integration plan (Phase 5).
}
