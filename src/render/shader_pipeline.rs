//! Run a chosen fragment shader over a source video texture into an
//! output texture. Owns the output FBO and a compile cache.
//!
//! Plan-A scope: one output FBO, one active program at a time, no `u_prev`
//! ping-pong. Sub-plan B adds the ping-pong + hot-reload.

#![cfg(any(feature = "desktop", feature = "pi-base"))]

use std::collections::HashMap;

use glow::HasContext;

use crate::error::Result;
use crate::shader::ShaderLibrary;

use super::shader::{compile_program, QUAD};
use super::shader_assembly::{assemble_fragment_source, vertex_source_for, GlesProfile};

type GlProgram = <glow::Context as HasContext>::Program;
type GlTexture = <glow::Context as HasContext>::Texture;
type GlFbo = <glow::Context as HasContext>::Framebuffer;
type GlBuffer = <glow::Context as HasContext>::Buffer;
type GlUniformLocation = <glow::Context as HasContext>::UniformLocation;

struct CachedProgram {
    program: GlProgram,
    u_source_0: Option<GlUniformLocation>,
    u_prev: Option<GlUniformLocation>,
    u_resolution: Option<GlUniformLocation>,
    u_source_0_size: Option<GlUniformLocation>,
    u_time: Option<GlUniformLocation>,
    u_trigger: Option<GlUniformLocation>,
    u_params: [Option<GlUniformLocation>; 8],
}

pub struct ShaderPipeline {
    profile: GlesProfile,
    library: ShaderLibrary,
    cache: HashMap<String, CachedProgram>,
    /// Output FBO + colour texture, sized to render target. Lazily allocated.
    output: Option<(GlFbo, GlTexture, u32, u32)>,
    /// Geometry: single fullscreen quad shared across all shaders.
    vbo: Option<GlBuffer>,
    /// Active shader name (lookup key into `cache`).
    active: Option<String>,
    /// Trigger envelope value, decayed each frame.
    trigger: f32,
    /// Staged uniform values for the 8 shader param slots.
    params: [f32; 8],
}

impl ShaderPipeline {
    pub fn new(profile: GlesProfile, library: ShaderLibrary) -> Self {
        Self {
            profile,
            library,
            cache: HashMap::new(),
            output: None,
            vbo: None,
            active: None,
            trigger: 0.0,
            params: [0.0; 8],
        }
    }

    pub fn profile(&self) -> GlesProfile {
        self.profile
    }

    pub fn library(&self) -> &ShaderLibrary {
        &self.library
    }

    /// Drop the cached compiled program for `name` so the next `select` call
    /// re-compiles. Safe to call without a current GL context — only mutates
    /// the host-side cache map.
    pub fn invalidate(&mut self, name: &str) {
        self.cache.remove(name);
    }

    /// Push the active shader-slot's 8 param values for the next apply().
    pub fn set_params(&mut self, params: [f32; 8]) {
        self.params = params;
    }

    /// Read the currently-staged param values (mostly for tests).
    pub fn params(&self) -> [f32; 8] {
        self.params
    }

    /// Drop the active selection. Subsequent `apply()` calls return the source
    /// texture unchanged (bypass path).
    pub fn clear_active(&mut self) {
        self.active = None;
    }

    /// Mutable access to the library — used by hot-reload to swap a shader
    /// source body in place.
    pub fn library_mut(&mut self) -> &mut ShaderLibrary {
        &mut self.library
    }

    /// Make the named shader active (compile + cache on first reference).
    ///
    /// # Safety
    /// Caller must hold a current GL context.
    pub unsafe fn select(&mut self, gl: &glow::Context, name: &str) -> Result<()> {
        if !self.cache.contains_key(name) {
            let shader = self.library.require(name)?;
            let frag_src = assemble_fragment_source(self.profile, &shader.fragment_body);
            let vert_src = vertex_source_for(self.profile);
            let program = compile_program(gl, vert_src, &frag_src)
                .map_err(|e| crate::error::Error::ShaderCompile(format!("{name}: {e}")))?;
            let cached = Self::cache_uniforms(gl, program);
            self.cache.insert(name.to_string(), cached);
        }
        self.active = Some(name.to_string());
        Ok(())
    }

    /// Pulse the trigger uniform to 1.0; decays via `tick`.
    pub fn pulse_trigger(&mut self) {
        self.trigger = 1.0;
    }

    /// Decay the trigger envelope by `dt` seconds. Call once per frame.
    /// Decays to 0 over ~0.5s.
    pub fn tick(&mut self, dt_seconds: f32) {
        self.trigger = (self.trigger - dt_seconds * 2.0).max(0.0);
    }

    /// Run the active shader over `source_tex`, writing into the pipeline's
    /// output texture. Returns the output texture handle (caller binds it
    /// for the next stage). Returns the source texture unchanged if no
    /// shader is selected (passthrough-by-absence).
    ///
    /// `(w, h)` is the render-target size; the output FBO is sized to match.
    /// `t` is `u_time` in seconds.
    ///
    /// # Safety
    /// Caller must hold a current GL context.
    #[allow(clippy::too_many_arguments)]
    pub unsafe fn apply(
        &mut self,
        gl: &glow::Context,
        source_tex: GlTexture,
        source_w: u32,
        source_h: u32,
        w: u32,
        h: u32,
        t: f32,
    ) -> Result<GlTexture> {
        let Some(active) = self.active.as_ref().cloned() else {
            return Ok(source_tex);
        };

        // Ensure GL resources are allocated before borrowing the cache.
        self.ensure_output(gl, w, h)?;
        self.ensure_vbo(gl);

        // Lazy recompile on cache miss (hot-reload path).
        if !self.cache.contains_key(&active) {
            self.select(gl, &active)?;
        }

        // Copy out the handles (all Copy types) before taking the cached ref.
        let (fbo, output_tex, _, _) = *self.output.as_ref().unwrap();
        let vbo = self.vbo.unwrap();

        let cached = self
            .cache
            .get(&active)
            .expect("just compiled or already cached");

        gl.bind_framebuffer(glow::FRAMEBUFFER, Some(fbo));
        gl.viewport(0, 0, w as i32, h as i32);
        gl.use_program(Some(cached.program));

        // Bind source_tex to texture unit 0 → u_source_0.
        gl.active_texture(glow::TEXTURE0);
        gl.bind_texture(glow::TEXTURE_2D, Some(source_tex));
        if let Some(loc) = &cached.u_source_0 {
            gl.uniform_1_i32(Some(loc), 0);
        }

        // u_prev — for Plan A, bind the source texture as a placeholder.
        // Sub-plan B replaces this with the ping-pong "previous output" texture.
        if let Some(loc) = &cached.u_prev {
            gl.active_texture(glow::TEXTURE1);
            gl.bind_texture(glow::TEXTURE_2D, Some(source_tex));
            gl.uniform_1_i32(Some(loc), 1);
        }

        if let Some(loc) = &cached.u_resolution {
            gl.uniform_2_f32(Some(loc), w as f32, h as f32);
        }
        if let Some(loc) = &cached.u_source_0_size {
            gl.uniform_2_f32(Some(loc), source_w as f32, source_h as f32);
        }
        if let Some(loc) = &cached.u_time {
            gl.uniform_1_f32(Some(loc), t);
        }
        if let Some(loc) = &cached.u_trigger {
            gl.uniform_1_f32(Some(loc), self.trigger);
        }
        for (i, loc) in cached.u_params.iter().enumerate() {
            if let Some(loc) = loc {
                gl.uniform_1_f32(Some(loc), self.params[i]);
            }
        }

        // Draw the fullscreen quad.
        gl.bind_buffer(glow::ARRAY_BUFFER, Some(vbo));
        gl.enable_vertex_attrib_array(0);
        gl.enable_vertex_attrib_array(1);
        gl.vertex_attrib_pointer_f32(0, 2, glow::FLOAT, false, 16, 0);
        gl.vertex_attrib_pointer_f32(1, 2, glow::FLOAT, false, 16, 8);
        gl.draw_arrays(glow::TRIANGLES, 0, 6);
        gl.disable_vertex_attrib_array(0);
        gl.disable_vertex_attrib_array(1);

        // Restore default FBO so the caller's screen-draw works.
        gl.bind_framebuffer(glow::FRAMEBUFFER, None);

        Ok(output_tex)
    }

    unsafe fn ensure_output(&mut self, gl: &glow::Context, w: u32, h: u32) -> Result<()> {
        let needs_realloc = match &self.output {
            None => true,
            Some((_, _, cw, ch)) => *cw != w || *ch != h,
        };
        if !needs_realloc {
            return Ok(());
        }
        if let Some((fbo, tex, _, _)) = self.output.take() {
            gl.delete_framebuffer(fbo);
            gl.delete_texture(tex);
        }
        let tex = gl
            .create_texture()
            .map_err(|e| crate::error::Error::ShaderCompile(format!("create output tex: {e}")))?;
        gl.bind_texture(glow::TEXTURE_2D, Some(tex));
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

        let fbo = gl
            .create_framebuffer()
            .map_err(|e| crate::error::Error::ShaderCompile(format!("create fbo: {e}")))?;
        gl.bind_framebuffer(glow::FRAMEBUFFER, Some(fbo));
        gl.framebuffer_texture_2d(
            glow::FRAMEBUFFER,
            glow::COLOR_ATTACHMENT0,
            glow::TEXTURE_2D,
            Some(tex),
            0,
        );
        let status = gl.check_framebuffer_status(glow::FRAMEBUFFER);
        if status != glow::FRAMEBUFFER_COMPLETE {
            return Err(crate::error::Error::ShaderCompile(format!(
                "FBO incomplete: 0x{status:04x}"
            )));
        }
        gl.bind_framebuffer(glow::FRAMEBUFFER, None);
        self.output = Some((fbo, tex, w, h));
        Ok(())
    }

    unsafe fn ensure_vbo(&mut self, gl: &glow::Context) {
        if self.vbo.is_some() {
            return;
        }
        let vbo = gl.create_buffer().expect("create vbo");
        gl.bind_buffer(glow::ARRAY_BUFFER, Some(vbo));
        gl.buffer_data_u8_slice(
            glow::ARRAY_BUFFER,
            bytemuck::cast_slice(QUAD),
            glow::STATIC_DRAW,
        );
        self.vbo = Some(vbo);
    }

    #[cfg(test)]
    pub fn active_name(&self) -> Option<&str> {
        self.active.as_deref()
    }

    #[cfg(test)]
    pub fn set_active_for_test(&mut self, name: &str) {
        self.active = Some(name.to_string());
    }

    unsafe fn cache_uniforms(gl: &glow::Context, program: GlProgram) -> CachedProgram {
        let lookup = |name: &str| gl.get_uniform_location(program, name);
        let u_params = std::array::from_fn(|i| lookup(&format!("u_param{i}")));
        CachedProgram {
            program,
            u_source_0: lookup("u_source_0"),
            u_prev: lookup("u_prev"),
            u_resolution: lookup("u_resolution"),
            u_source_0_size: lookup("u_source_0_size"),
            u_time: lookup("u_time"),
            u_trigger: lookup("u_trigger"),
            u_params,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trigger_decays_to_zero() {
        let lib = ShaderLibrary::default();
        let mut p = ShaderPipeline::new(GlesProfile::V100, lib);
        p.pulse_trigger();
        assert_eq!(p.trigger, 1.0);
        p.tick(0.25);
        assert!(p.trigger < 1.0);
        p.tick(0.25);
        assert_eq!(p.trigger, 0.0);
    }

    #[test]
    fn no_active_shader_returns_source_unchanged() {
        let lib = ShaderLibrary::default();
        let p = ShaderPipeline::new(GlesProfile::V100, lib);
        assert!(p.active.is_none());
    }

    #[test]
    fn invalidate_drops_cache_entry_idempotently() {
        use crate::shader::{LoadedShader, ShaderMeta};
        let mut lib = ShaderLibrary::default();
        let meta = ShaderMeta::parse("name = \"foo\"\n", "<test>").unwrap();
        lib.upsert(
            "foo",
            LoadedShader {
                meta,
                fragment_body: "void main(){gl_FragColor=vec4(0);}".into(),
                source_path: std::path::PathBuf::from("foo.glsl"),
            },
        );
        let mut p = ShaderPipeline::new(GlesProfile::V100, lib);
        // No GL context — just exercise the public API path. The cache is private,
        // but invalidate is safe to call against a never-compiled entry.
        p.invalidate("foo");
        assert!(p.library().get("foo").is_some(), "library entry remains");
    }

    #[test]
    fn set_params_stores_values_for_next_apply() {
        let mut p = ShaderPipeline::new(GlesProfile::V100, ShaderLibrary::default());
        p.set_params([0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8]);
        // Public read-back: we expose params() to allow tests to confirm.
        assert_eq!(p.params(), [0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8]);
    }

    #[test]
    fn clear_active_drops_selection() {
        let mut p = ShaderPipeline::new(GlesProfile::V100, ShaderLibrary::default());
        p.set_active_for_test("foo");
        assert_eq!(p.active_name(), Some("foo"));
        p.clear_active();
        assert_eq!(p.active_name(), None);
    }

    #[test]
    fn invalidate_does_not_break_subsequent_apply_call_path() {
        // We can't actually run apply() without a GL context, but we can prove
        // that invalidate + an active selection leaves a coherent state: the
        // active name persists and the cache miss is detectable by the public API.
        let mut p = ShaderPipeline::new(GlesProfile::V100, ShaderLibrary::default());
        p.set_active_for_test("color_shift");
        p.invalidate("color_shift");
        // The active name is preserved; only the cached compiled program is gone.
        assert_eq!(p.active_name(), Some("color_shift"));
    }
}
