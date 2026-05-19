//! GL text overlay: renders a `TextGrid` of menu/status text on top of the
//! video layer.
//!
//! At startup we bake a 95-char ASCII glyph atlas (chars 32..127) using
//! `embedded_graphics::mono_font::ascii::FONT_6X10` into an in-memory RGBA
//! buffer and upload it as a single GL texture. Each frame we emit one
//! textured quad per non-blank cell in the grid into a dynamic VBO and issue
//! one draw call.
//!
//! Layout choices:
//! - Glyph cell = 6×10 px. Atlas = 16×6 cells = 96×60 px.
//! - Character `n` (0..96 for chars 32..127 + a blank slot at 95) lives at
//!   atlas cell `(n % 16, n / 16)`.
//! - The grid is drawn as a full-window overlay; each cell scales to
//!   `(viewport_w / cols, viewport_h / rows)` pixels. The video bleeds
//!   through where atlas alpha is 0, plus an `overlay_alpha` tint on the
//!   colored background.
//!
//! Color theme is intentionally minimal: amber-on-black, matching the
//! intended SPI panel look. Inverse cells swap fg/bg per-vertex on the CPU.

use embedded_graphics::image::Image;
use embedded_graphics::mono_font::ascii::FONT_6X10;
use embedded_graphics::mono_font::MonoFont;
use embedded_graphics::pixelcolor::BinaryColor;
use embedded_graphics::prelude::*;
use glow::HasContext;

use crate::status::grid::{TextGrid, ATTR_DIM, ATTR_INVERSE};

// ── Atlas geometry ────────────────────────────────────────────────────────────

pub const GLYPH_W: u32 = 6;
pub const GLYPH_H: u32 = 10;

/// 16 glyphs across, 6 down → 96 cells. We use the first 95 for ASCII 32..=126,
/// the last cell stays blank.
pub const ATLAS_COLS: u32 = 16;
pub const ATLAS_ROWS: u32 = 6;
pub const ATLAS_W: u32 = ATLAS_COLS * GLYPH_W; // 96
pub const ATLAS_H: u32 = ATLAS_ROWS * GLYPH_H; // 60

/// Map an ASCII char to its atlas cell index. Non-printable / out-of-range →
/// last (blank) cell.
fn glyph_index(ch: char) -> usize {
    let c = ch as u32;
    if (32..=126).contains(&c) {
        (c - 32) as usize
    } else {
        95
    }
}

// ── Atlas baking ──────────────────────────────────────────────────────────────

/// In-memory RGBA buffer used as a `DrawTarget` for glyph rasterisation.
/// Stores white pixels where the font is on, transparent elsewhere.
struct AtlasFb {
    width: u32,
    height: u32,
    data: Vec<u8>, // RGBA, row-major
}

impl AtlasFb {
    fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            data: vec![0u8; (width * height * 4) as usize],
        }
    }

    fn set_pixel(&mut self, x: i32, y: i32, on: bool) {
        if x < 0 || y < 0 {
            return;
        }
        let (x, y) = (x as u32, y as u32);
        if x >= self.width || y >= self.height {
            return;
        }
        let i = ((y * self.width + x) * 4) as usize;
        let v = if on { 255 } else { 0 };
        self.data[i] = v;
        self.data[i + 1] = v;
        self.data[i + 2] = v;
        self.data[i + 3] = v;
    }
}

impl OriginDimensions for AtlasFb {
    fn size(&self) -> Size {
        Size::new(self.width, self.height)
    }
}

impl DrawTarget for AtlasFb {
    type Color = BinaryColor;
    type Error = core::convert::Infallible;

    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Pixel<Self::Color>>,
    {
        for Pixel(p, c) in pixels {
            self.set_pixel(p.x, p.y, c.is_on());
        }
        Ok(())
    }
}

/// Build the 96×60 RGBA glyph atlas.
fn bake_atlas() -> Vec<u8> {
    let font: &MonoFont = &FONT_6X10;
    let mut fb = AtlasFb::new(ATLAS_W, ATLAS_H);

    // Pull each ASCII char by drawing the font's sub-image into the atlas at
    // the matching grid cell. We bypass `Text` (which would need to allocate a
    // String per glyph) and instead use the font's raw image plus the public
    // `glyph_mapping` to find sub-image rects.
    //
    // `font.image` is the full bitmap of all glyphs; `font.glyph_mapping` maps
    // a char to its index in that bitmap. The image is laid out as a single
    // strip (or multi-row strip) of glyphs of size `character_size`. Width is
    // `font.image.size().width`.
    let img_w = font.image.size().width;
    let glyphs_per_row = img_w / font.character_size.width;

    for code in 32u32..=126 {
        let ch = char::from_u32(code).unwrap();
        let dest_idx = (code - 32) as usize;
        let dest_col = (dest_idx as u32) % ATLAS_COLS;
        let dest_row = (dest_idx as u32) / ATLAS_COLS;
        let dest_x = (dest_col * GLYPH_W) as i32;
        let dest_y = (dest_row * GLYPH_H) as i32;

        let src_idx = font.glyph_mapping.index(ch) as u32;
        let src_col = src_idx % glyphs_per_row;
        let src_row = src_idx / glyphs_per_row;
        let src_x = src_col * font.character_size.width;
        let src_y = src_row * font.character_size.height;

        // Copy the glyph's bits into the atlas. We re-implement the lookup
        // against `ImageRaw` by drawing the whole image translated and then
        // clipping — but easier: walk pixels manually via `Image`.
        // Use `Image::new` + `draw` with translation. That fully respects the
        // image format (1bpp packed).
        let image = Image::new(
            &font.image,
            Point::new(dest_x - src_x as i32, dest_y - src_y as i32),
        );
        // Clip the draw to the destination glyph rect by using a translated
        // sub-target. Simpler: draw the whole thing — pixels outside our
        // (dest_x..dest_x+GLYPH_W, dest_y..dest_y+GLYPH_H) overwrite other
        // glyphs. So we need a clipped target. Implement clipping via a
        // wrapper.
        let mut clip = ClipFb {
            inner: &mut fb,
            x0: dest_x,
            y0: dest_y,
            x1: dest_x + GLYPH_W as i32,
            y1: dest_y + GLYPH_H as i32,
        };
        let _ = image.draw(&mut clip);
    }

    fb.data
}

/// `DrawTarget` wrapper that drops pixels outside `[x0,x1) × [y0,y1)`.
struct ClipFb<'a> {
    inner: &'a mut AtlasFb,
    x0: i32,
    y0: i32,
    x1: i32,
    y1: i32,
}

impl<'a> OriginDimensions for ClipFb<'a> {
    fn size(&self) -> Size {
        self.inner.size()
    }
}

impl<'a> DrawTarget for ClipFb<'a> {
    type Color = BinaryColor;
    type Error = core::convert::Infallible;

    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Pixel<Self::Color>>,
    {
        for Pixel(p, c) in pixels {
            if p.x >= self.x0 && p.x < self.x1 && p.y >= self.y0 && p.y < self.y1 {
                self.inner.set_pixel(p.x, p.y, c.is_on());
            }
        }
        Ok(())
    }
}

// ── GL overlay ────────────────────────────────────────────────────────────────

pub const VERT_SRC: &str = r#"
#version 100
attribute vec2 a_pos;
attribute vec2 a_uv;
attribute vec3 a_fg;
attribute vec3 a_bg;
varying vec2 v_uv;
varying vec3 v_fg;
varying vec3 v_bg;
void main() {
    v_uv = a_uv;
    v_fg = a_fg;
    v_bg = a_bg;
    gl_Position = vec4(a_pos, 0.0, 1.0);
}
"#;

pub const FRAG_SRC: &str = r#"
#version 100
precision mediump float;
varying vec2 v_uv;
varying vec3 v_fg;
varying vec3 v_bg;
uniform sampler2D u_atlas;
uniform float u_alpha;
void main() {
    float m = texture2D(u_atlas, v_uv).a;
    vec3 col = mix(v_bg, v_fg, m);
    // The cell's contribution to the screen: opaque foreground where the
    // glyph is on, semi-transparent background elsewhere. This lets video
    // bleed through the empty regions of the overlay.
    float a = mix(u_alpha, 1.0, m);
    gl_FragColor = vec4(col, a);
}
"#;

const FLOATS_PER_VERT: usize = 10; // x,y, u,v, fr,fg,fb, br,bg,bb
const VERTS_PER_CELL: usize = 6;

/// Persistent GL state for the text overlay.
pub struct TextOverlay {
    program: <glow::Context as HasContext>::Program,
    vbo: <glow::Context as HasContext>::Buffer,
    atlas_tex: <glow::Context as HasContext>::Texture,
    u_atlas: Option<<glow::Context as HasContext>::UniformLocation>,
    u_alpha: Option<<glow::Context as HasContext>::UniformLocation>,
    a_pos: u32,
    a_uv: u32,
    a_fg: u32,
    a_bg: u32,
    vbo_capacity_floats: usize,
    /// CPU-side scratch buffer; grown lazily.
    scratch: Vec<f32>,
}

impl TextOverlay {
    /// Build the overlay GL objects. Requires a current GL context.
    ///
    /// # Safety
    /// Caller must hold a current GL context.
    pub unsafe fn new(gl: &glow::Context) -> anyhow::Result<Self> {
        let program = super::shader::compile_program(gl, VERT_SRC, FRAG_SRC)?;

        let vbo = gl
            .create_buffer()
            .map_err(|e| anyhow::anyhow!("text vbo: {e}"))?;

        // Atlas texture
        let atlas_tex = gl
            .create_texture()
            .map_err(|e| anyhow::anyhow!("text atlas: {e}"))?;
        let atlas_rgba = bake_atlas();
        gl.bind_texture(glow::TEXTURE_2D, Some(atlas_tex));
        gl.tex_image_2d(
            glow::TEXTURE_2D,
            0,
            glow::RGBA as i32,
            ATLAS_W as i32,
            ATLAS_H as i32,
            0,
            glow::RGBA,
            glow::UNSIGNED_BYTE,
            Some(&atlas_rgba),
        );
        gl.tex_parameter_i32(
            glow::TEXTURE_2D,
            glow::TEXTURE_MIN_FILTER,
            glow::NEAREST as i32,
        );
        gl.tex_parameter_i32(
            glow::TEXTURE_2D,
            glow::TEXTURE_MAG_FILTER,
            glow::NEAREST as i32,
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

        let u_atlas = gl.get_uniform_location(program, "u_atlas");
        let u_alpha = gl.get_uniform_location(program, "u_alpha");
        let a_pos = gl
            .get_attrib_location(program, "a_pos")
            .ok_or_else(|| anyhow::anyhow!("a_pos not found"))?;
        let a_uv = gl
            .get_attrib_location(program, "a_uv")
            .ok_or_else(|| anyhow::anyhow!("a_uv not found"))?;
        let a_fg = gl
            .get_attrib_location(program, "a_fg")
            .ok_or_else(|| anyhow::anyhow!("a_fg not found"))?;
        let a_bg = gl
            .get_attrib_location(program, "a_bg")
            .ok_or_else(|| anyhow::anyhow!("a_bg not found"))?;

        Ok(Self {
            program,
            vbo,
            atlas_tex,
            u_atlas,
            u_alpha,
            a_pos,
            a_uv,
            a_fg,
            a_bg,
            vbo_capacity_floats: 0,
            scratch: Vec::new(),
        })
    }

    /// Render `grid` to the full viewport at the given background overlay
    /// alpha (0..1). Glyphs themselves are fully opaque so they remain
    /// readable on top of the video.
    ///
    /// # Safety
    /// Caller must hold a current GL context.
    pub unsafe fn draw(&mut self, gl: &glow::Context, grid: &TextGrid, overlay_alpha: f32) {
        // Build vertex array.
        let cols = grid.cols.max(1) as f32;
        let rows = grid.rows.max(1) as f32;
        let cell_w_ndc = 2.0 / cols;
        let cell_h_ndc = 2.0 / rows;

        // Amber on black.
        const FG: [f32; 3] = [1.0, 0.66, 0.0];
        const BG: [f32; 3] = [0.0, 0.0, 0.0];

        self.scratch.clear();
        let cap_floats = grid.cols * grid.rows * VERTS_PER_CELL * FLOATS_PER_VERT;
        self.scratch.reserve(cap_floats);

        for row in 0..grid.rows {
            for col in 0..grid.cols {
                let cell = grid.at(row, col);
                let idx = glyph_index(cell.ch);

                let (fg, bg) = if cell.attr & ATTR_INVERSE != 0 {
                    (BG, FG)
                } else {
                    (FG, BG)
                };
                // ATTR_DIM mixes fg toward bg by 60% → glyph reads at ~40% brightness.
                // Skip dim when ATTR_INVERSE is also set: the inversion already
                // distinguishes the selected row; combining both would produce
                // near-zero contrast (dark-amber-on-amber).
                let (fg, bg) = if cell.attr & ATTR_DIM != 0 && cell.attr & ATTR_INVERSE == 0 {
                    let mix = |a: [f32; 3], b: [f32; 3]| {
                        [
                            a[0] * 0.4 + b[0] * 0.6,
                            a[1] * 0.4 + b[1] * 0.6,
                            a[2] * 0.4 + b[2] * 0.6,
                        ]
                    };
                    (mix(fg, bg), bg)
                } else {
                    (fg, bg)
                };

                // NDC: x left → right, y top → bottom. We flip y so row 0 is
                // at the top of the window.
                let x0 = -1.0 + col as f32 * cell_w_ndc;
                let x1 = x0 + cell_w_ndc;
                let y0 = 1.0 - row as f32 * cell_h_ndc;
                let y1 = y0 - cell_h_ndc;

                // UVs into the atlas. Cell `idx` lives at (idx % ATLAS_COLS,
                // idx / ATLAS_COLS).
                let ac = (idx as u32 % ATLAS_COLS) as f32;
                let ar = (idx as u32 / ATLAS_COLS) as f32;
                let u0 = ac / ATLAS_COLS as f32;
                let u1 = (ac + 1.0) / ATLAS_COLS as f32;
                let v0 = ar / ATLAS_ROWS as f32;
                let v1 = (ar + 1.0) / ATLAS_ROWS as f32;

                // Six vertices: tri (TL, BL, TR), tri (TR, BL, BR).
                // y0 = top, y1 = bottom. v0 = top in atlas, v1 = bottom.
                let verts = [
                    (x0, y0, u0, v0), // TL
                    (x0, y1, u0, v1), // BL
                    (x1, y0, u1, v0), // TR
                    (x1, y0, u1, v0), // TR
                    (x0, y1, u0, v1), // BL
                    (x1, y1, u1, v1), // BR
                ];
                for (vx, vy, uu, vv) in verts {
                    self.scratch.push(vx);
                    self.scratch.push(vy);
                    self.scratch.push(uu);
                    self.scratch.push(vv);
                    self.scratch.push(fg[0]);
                    self.scratch.push(fg[1]);
                    self.scratch.push(fg[2]);
                    self.scratch.push(bg[0]);
                    self.scratch.push(bg[1]);
                    self.scratch.push(bg[2]);
                }
            }
        }

        // Upload VBO. Grow with orphaning if needed, else sub-data.
        gl.bind_buffer(glow::ARRAY_BUFFER, Some(self.vbo));
        let bytes: &[u8] = bytemuck::cast_slice(&self.scratch);
        if self.scratch.len() > self.vbo_capacity_floats {
            gl.buffer_data_u8_slice(glow::ARRAY_BUFFER, bytes, glow::DYNAMIC_DRAW);
            self.vbo_capacity_floats = self.scratch.len();
        } else {
            gl.buffer_sub_data_u8_slice(glow::ARRAY_BUFFER, 0, bytes);
        }

        // Setup pipeline state.
        gl.use_program(Some(self.program));
        gl.active_texture(glow::TEXTURE0);
        gl.bind_texture(glow::TEXTURE_2D, Some(self.atlas_tex));
        gl.uniform_1_i32(self.u_atlas.as_ref(), 0);
        gl.uniform_1_f32(self.u_alpha.as_ref(), overlay_alpha);

        gl.enable(glow::BLEND);
        gl.blend_func(glow::SRC_ALPHA, glow::ONE_MINUS_SRC_ALPHA);

        let stride = (FLOATS_PER_VERT * 4) as i32;
        gl.enable_vertex_attrib_array(self.a_pos);
        gl.vertex_attrib_pointer_f32(self.a_pos, 2, glow::FLOAT, false, stride, 0);
        gl.enable_vertex_attrib_array(self.a_uv);
        gl.vertex_attrib_pointer_f32(self.a_uv, 2, glow::FLOAT, false, stride, 8);
        gl.enable_vertex_attrib_array(self.a_fg);
        gl.vertex_attrib_pointer_f32(self.a_fg, 3, glow::FLOAT, false, stride, 16);
        gl.enable_vertex_attrib_array(self.a_bg);
        gl.vertex_attrib_pointer_f32(self.a_bg, 3, glow::FLOAT, false, stride, 28);

        let count = (self.scratch.len() / FLOATS_PER_VERT) as i32;
        gl.draw_arrays(glow::TRIANGLES, 0, count);

        gl.disable_vertex_attrib_array(self.a_pos);
        gl.disable_vertex_attrib_array(self.a_uv);
        gl.disable_vertex_attrib_array(self.a_fg);
        gl.disable_vertex_attrib_array(self.a_bg);
        gl.disable(glow::BLEND);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn glyph_index_maps_space_to_zero() {
        assert_eq!(glyph_index(' '), 0);
    }

    #[test]
    fn glyph_index_maps_capital_a() {
        assert_eq!(glyph_index('A'), 'A' as usize - 32);
    }

    #[test]
    fn glyph_index_out_of_range_falls_back() {
        assert_eq!(glyph_index('\0'), 95);
        assert_eq!(glyph_index('€'), 95);
    }

    #[test]
    fn atlas_dimensions_match_constants() {
        let atlas = bake_atlas();
        assert_eq!(atlas.len() as u32, ATLAS_W * ATLAS_H * 4);
    }

    #[test]
    fn baked_atlas_contains_some_glyph_pixels() {
        // 'A' lives at index 33 → cell col 1, row 2. Expect at least one
        // opaque pixel in that cell.
        let atlas = bake_atlas();
        let cell_x = (('A' as u32 - 32) % ATLAS_COLS) * GLYPH_W;
        let cell_y = (('A' as u32 - 32) / ATLAS_COLS) * GLYPH_H;
        let mut any = false;
        for y in cell_y..(cell_y + GLYPH_H) {
            for x in cell_x..(cell_x + GLYPH_W) {
                let i = ((y * ATLAS_W + x) * 4 + 3) as usize;
                if atlas[i] != 0 {
                    any = true;
                }
            }
        }
        assert!(any, "expected at least one lit pixel for 'A' in atlas");
    }
}
