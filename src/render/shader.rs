//! GLSL ES 1.00 shader pair for the fullscreen video quad, plus shared compile helpers.

use glow::HasContext;

pub const VERT: &str = r#"
#version 100
attribute vec2 a_pos;
attribute vec2 a_uv;
varying vec2 v_uv;
void main() {
    v_uv = a_uv;
    gl_Position = vec4(a_pos, 0.0, 1.0);
}
"#;

pub const FRAG: &str = r#"
#version 100
precision mediump float;
varying vec2 v_uv;
uniform sampler2D u_tex;
uniform float u_alpha;
void main() {
    vec4 c = texture2D(u_tex, v_uv);
    gl_FragColor = vec4(c.rgb, c.a * u_alpha);
}
"#;

// Two-triangle quad covering NDC [-1,1] with flipped V (image top = GL bottom).
// Layout: (x, y, u, v)
pub const QUAD: &[f32] = &[
    -1.0, -1.0, 0.0, 1.0, 1.0, -1.0, 1.0, 1.0, -1.0, 1.0, 0.0, 0.0, -1.0, 1.0, 0.0, 0.0, 1.0, -1.0,
    1.0, 1.0, 1.0, 1.0, 1.0, 0.0,
];

/// Compile a single GLSL shader stage. Shared by desktop and Pi backends.
///
/// # Safety
/// Caller must hold a current GL context.
pub unsafe fn compile_shader(
    gl: &glow::Context,
    kind: u32,
    src: &str,
) -> anyhow::Result<<glow::Context as glow::HasContext>::Shader> {
    let s = gl
        .create_shader(kind)
        .map_err(|e| anyhow::anyhow!("create shader: {e}"))?;
    gl.shader_source(s, src);
    gl.compile_shader(s);
    if !gl.get_shader_compile_status(s) {
        let log = gl.get_shader_info_log(s);
        gl.delete_shader(s);
        return Err(anyhow::anyhow!("shader compile: {log}"));
    }
    Ok(s)
}

/// Link a vertex + fragment shader into a program. Shared by desktop and Pi backends.
///
/// # Safety
/// Caller must hold a current GL context.
pub unsafe fn compile_program(
    gl: &glow::Context,
    vert_src: &str,
    frag_src: &str,
) -> anyhow::Result<<glow::Context as glow::HasContext>::Program> {
    let v = compile_shader(gl, glow::VERTEX_SHADER, vert_src)?;
    let f = compile_shader(gl, glow::FRAGMENT_SHADER, frag_src)?;

    let prog = gl
        .create_program()
        .map_err(|e| anyhow::anyhow!("create program: {e}"))?;

    gl.attach_shader(prog, v);
    gl.attach_shader(prog, f);
    gl.bind_attrib_location(prog, 0, "a_pos");
    gl.bind_attrib_location(prog, 1, "a_uv");
    gl.link_program(prog);

    if !gl.get_program_link_status(prog) {
        let log = gl.get_program_info_log(prog);
        gl.delete_program(prog);
        gl.delete_shader(v);
        gl.delete_shader(f);
        return Err(anyhow::anyhow!("shader link: {log}"));
    }

    gl.detach_shader(prog, v);
    gl.detach_shader(prog, f);
    gl.delete_shader(v);
    gl.delete_shader(f);

    Ok(prog)
}
