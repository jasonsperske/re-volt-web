//! WebGL2 renderer for scenes built by `crate::scene`.

use std::collections::BTreeMap;
use std::rc::Rc;

use glam::{Mat4, Vec3};
use wasm_bindgen::{JsCast, JsValue};
use web_sys::{
    HtmlCanvasElement, WebGl2RenderingContext as Gl, WebGlBuffer, WebGlFramebuffer, WebGlProgram, WebGlShader, WebGlTexture,
    WebGlUniformLocation, WebGlVertexArrayObject,
};

use crate::formats::bmp::Image;
use crate::scene::{BatchKey, Blend, SceneBuilder, VERTEX_SIZE};

const VERTEX_SHADER: &str = r#"#version 300 es
layout(location = 0) in vec3 a_pos;
layout(location = 1) in vec2 a_uv;
layout(location = 2) in vec4 a_color;
uniform mat4 u_view_proj;
uniform mat4 u_model;
uniform vec3 u_eye;
out vec2 v_uv;
out vec4 v_color;
out float v_dist;
void main() {
    vec4 world = u_model * vec4(a_pos, 1.0);
    gl_Position = u_view_proj * world;
    v_uv = a_uv;
    v_color = a_color;
    v_dist = distance(world.xyz, u_eye);
}
"#;

const FRAGMENT_SHADER: &str = r#"#version 300 es
precision mediump float;
uniform sampler2D u_tex;
uniform vec3 u_fog_color;
uniform vec2 u_fog_range;
in vec2 v_uv;
in vec4 v_color;
in float v_dist;
out vec4 o_color;
void main() {
    vec4 tex = texture(u_tex, v_uv);
    // Black is the colour key in Re-Volt textures (decoded to alpha 0).
    if (tex.a < 0.5) discard;
    vec4 c = tex * v_color;
    float fog = clamp((v_dist - u_fog_range.x) / max(u_fog_range.y - u_fog_range.x, 1.0), 0.0, 1.0);
    c.rgb = mix(c.rgb, u_fog_color, fog);
    o_color = c;
}
"#;

struct GpuBatch {
    key: BatchKey,
    vao: WebGlVertexArrayObject,
    buffer: WebGlBuffer,
    count: i32,
}

pub struct GpuScene {
    batches: Vec<GpuBatch>,
    /// Shared with other scenes when uploaded with `upload_shared`.
    textures: BTreeMap<i16, Rc<WebGlTexture>>,
}

pub struct Fog {
    pub color: [f32; 3],
    pub start: f32,
    pub end: f32,
}

pub struct Renderer {
    gl: Gl,
    canvas: HtmlCanvasElement,
    program: WebGlProgram,
    u_view_proj: Option<WebGlUniformLocation>,
    u_model: Option<WebGlUniformLocation>,
    u_eye: Option<WebGlUniformLocation>,
    u_fog_color: Option<WebGlUniformLocation>,
    u_fog_range: Option<WebGlUniformLocation>,
    white: WebGlTexture,
}

impl Renderer {
    pub fn new(canvas: HtmlCanvasElement) -> Result<Self, JsValue> {
        let gl: Gl = canvas
            .get_context("webgl2")?
            .ok_or("WebGL2 is not available in this browser")?
            .dyn_into()?;
        let program = link(&gl, VERTEX_SHADER, FRAGMENT_SHADER)?;
        let u = |name| gl.get_uniform_location(&program, name);
        let (u_view_proj, u_model, u_eye, u_fog_color, u_fog_range) =
            (u("u_view_proj"), u("u_model"), u("u_eye"), u("u_fog_color"), u("u_fog_range"));
        let white = upload_texture(&gl, &Image { width: 1, height: 1, rgba: vec![255; 4] })?;
        gl.enable(Gl::DEPTH_TEST);
        // Re-Volt polygons are clockwise when seen from the front.
        gl.front_face(Gl::CW);
        gl.cull_face(Gl::BACK);
        Ok(Renderer { gl, canvas, program, u_view_proj, u_model, u_eye, u_fog_color, u_fog_range, white })
    }

    /// Matches the drawing buffer to the canvas's CSS size; returns the aspect ratio.
    pub fn resize(&self) -> f32 {
        let dpr = web_sys::window().map(|w| w.device_pixel_ratio()).unwrap_or(1.0).min(2.0);
        let w = ((self.canvas.client_width() as f64 * dpr) as u32).max(1);
        let h = ((self.canvas.client_height() as f64 * dpr) as u32).max(1);
        if self.canvas.width() != w || self.canvas.height() != h {
            self.canvas.set_width(w);
            self.canvas.set_height(h);
        }
        self.gl.viewport(0, 0, w as i32, h as i32);
        w as f32 / h as f32
    }

    pub fn upload(&self, scene: &SceneBuilder, textures: &BTreeMap<i16, Image>) -> Result<GpuScene, JsValue> {
        let shared = self.upload_textures(textures)?;
        self.upload_shared(scene, &shared)
    }

    /// Uploads images once, for use by several scenes via `upload_shared`.
    pub fn upload_textures(&self, textures: &BTreeMap<i16, Image>) -> Result<BTreeMap<i16, Rc<WebGlTexture>>, JsValue> {
        textures.iter().map(|(slot, img)| Ok((*slot, Rc::new(upload_texture(&self.gl, img)?)))).collect()
    }

    /// Uploads geometry that draws with already-uploaded textures; they are
    /// freed when the last scene using them is destroyed.
    pub fn upload_shared(&self, scene: &SceneBuilder, textures: &BTreeMap<i16, Rc<WebGlTexture>>) -> Result<GpuScene, JsValue> {
        let gl = &self.gl;
        let mut batches = Vec::new();
        for (key, verts) in &scene.batches {
            if verts.is_empty() {
                continue;
            }
            let vao = gl.create_vertex_array().ok_or("create_vertex_array failed")?;
            let buffer = gl.create_buffer().ok_or("create_buffer failed")?;
            gl.bind_vertex_array(Some(&vao));
            gl.bind_buffer(Gl::ARRAY_BUFFER, Some(&buffer));
            // SAFETY: GpuVertex is repr(C) plain data.
            let bytes = unsafe { std::slice::from_raw_parts(verts.as_ptr() as *const u8, verts.len() * VERTEX_SIZE) };
            gl.buffer_data_with_u8_array(Gl::ARRAY_BUFFER, bytes, Gl::STATIC_DRAW);
            let stride = VERTEX_SIZE as i32;
            gl.enable_vertex_attrib_array(0);
            gl.vertex_attrib_pointer_with_i32(0, 3, Gl::FLOAT, false, stride, 0);
            gl.enable_vertex_attrib_array(1);
            gl.vertex_attrib_pointer_with_i32(1, 2, Gl::FLOAT, false, stride, 12);
            gl.enable_vertex_attrib_array(2);
            gl.vertex_attrib_pointer_with_i32(2, 4, Gl::UNSIGNED_BYTE, true, stride, 20);
            batches.push(GpuBatch { key: *key, vao, buffer, count: verts.len() as i32 });
        }
        gl.bind_vertex_array(None);
        // Only keep the textures this geometry uses.
        let used: BTreeMap<i16, Rc<WebGlTexture>> = scene
            .batches
            .keys()
            .filter_map(|k| k.texture)
            .filter_map(|t| textures.get(&t).map(|tex| (t, tex.clone())))
            .collect();
        Ok(GpuScene { batches, textures: used })
    }

    pub fn destroy(&self, scene: GpuScene) {
        for b in scene.batches {
            self.gl.delete_vertex_array(Some(&b.vao));
            self.gl.delete_buffer(Some(&b.buffer));
        }
        for t in scene.textures.into_values() {
            if let Ok(t) = Rc::try_unwrap(t) {
                self.gl.delete_texture(Some(&t));
            }
        }
    }

    /// Draws into `framebuffer` (a WebXR layer) instead of the canvas, or
    /// back to the canvas with `None`.
    pub fn bind_framebuffer(&self, framebuffer: Option<&WebGlFramebuffer>) {
        self.gl.bind_framebuffer(Gl::FRAMEBUFFER, framebuffer);
    }

    pub fn set_viewport(&self, [x, y, w, h]: [i32; 4]) {
        self.gl.viewport(x, y, w, h);
    }

    /// Copies a 2-D canvas into a texture (creating it when `into` is
    /// `None`), for overlays drawn in the 3-D scene.
    pub fn canvas_texture(&self, canvas: &HtmlCanvasElement, into: Option<&WebGlTexture>) -> Result<Option<WebGlTexture>, JsValue> {
        let gl = &self.gl;
        let created = match into {
            Some(_) => None,
            None => Some(gl.create_texture().ok_or("create_texture failed")?),
        };
        let tex = into.or(created.as_ref()).expect("texture");
        gl.bind_texture(Gl::TEXTURE_2D, Some(tex));
        gl.tex_image_2d_with_u32_and_u32_and_html_canvas_element(Gl::TEXTURE_2D, 0, Gl::RGBA as i32, Gl::RGBA, Gl::UNSIGNED_BYTE, canvas)?;
        gl.tex_parameteri(Gl::TEXTURE_2D, Gl::TEXTURE_MIN_FILTER, Gl::LINEAR as i32);
        gl.tex_parameteri(Gl::TEXTURE_2D, Gl::TEXTURE_MAG_FILTER, Gl::LINEAR as i32);
        gl.tex_parameteri(Gl::TEXTURE_2D, Gl::TEXTURE_WRAP_S, Gl::CLAMP_TO_EDGE as i32);
        gl.tex_parameteri(Gl::TEXTURE_2D, Gl::TEXTURE_WRAP_T, Gl::CLAMP_TO_EDGE as i32);
        Ok(created)
    }

    /// Like [`Renderer::draw`], but over everything already drawn.
    pub fn draw_on_top(&self, scenes: &[(&GpuScene, Mat4)], view_proj: Mat4, eye: Vec3, fog: &Fog) {
        self.gl.disable(Gl::DEPTH_TEST);
        self.draw(scenes, view_proj, eye, fog);
        self.gl.enable(Gl::DEPTH_TEST);
    }

    pub fn begin(&self, fog: &Fog) {
        let gl = &self.gl;
        gl.clear_color(fog.color[0], fog.color[1], fog.color[2], 1.0);
        gl.depth_mask(true);
        gl.clear(Gl::COLOR_BUFFER_BIT | Gl::DEPTH_BUFFER_BIT);
    }

    /// Draws scenes in passes: all opaque geometry first, then blended geometry
    /// without depth writes so translucent surfaces don't hide each other.
    pub fn draw(&self, scenes: &[(&GpuScene, Mat4)], view_proj: Mat4, eye: Vec3, fog: &Fog) {
        let gl = &self.gl;
        gl.use_program(Some(&self.program));
        gl.uniform_matrix4fv_with_f32_array(self.u_view_proj.as_ref(), false, &view_proj.to_cols_array());
        gl.uniform3f(self.u_eye.as_ref(), eye.x, eye.y, eye.z);
        gl.uniform2f(self.u_fog_range.as_ref(), fog.start, fog.end);
        gl.active_texture(Gl::TEXTURE0);

        for pass in [Blend::Opaque, Blend::Alpha, Blend::Additive] {
            match pass {
                Blend::Opaque => {
                    gl.disable(Gl::BLEND);
                    gl.depth_mask(true);
                }
                Blend::Alpha => {
                    gl.enable(Gl::BLEND);
                    gl.blend_func(Gl::SRC_ALPHA, Gl::ONE_MINUS_SRC_ALPHA);
                    gl.depth_mask(false);
                }
                Blend::Additive => {
                    gl.enable(Gl::BLEND);
                    gl.blend_func(Gl::ONE, Gl::ONE);
                    gl.depth_mask(false);
                }
            }
            // Additive light fades to nothing in fog rather than to the fog colour.
            let fc = if pass == Blend::Additive { [0.0; 3] } else { fog.color };
            gl.uniform3f(self.u_fog_color.as_ref(), fc[0], fc[1], fc[2]);
            for (scene, model) in scenes {
                gl.uniform_matrix4fv_with_f32_array(self.u_model.as_ref(), false, &model.to_cols_array());
                for b in scene.batches.iter().filter(|b| b.key.blend == pass) {
                    if b.key.double_sided {
                        gl.disable(Gl::CULL_FACE);
                    } else {
                        gl.enable(Gl::CULL_FACE);
                    }
                    let tex = b.key.texture.and_then(|t| scene.textures.get(&t)).map_or(&self.white, |t| &**t);
                    gl.bind_texture(Gl::TEXTURE_2D, Some(tex));
                    gl.bind_vertex_array(Some(&b.vao));
                    gl.draw_arrays(Gl::TRIANGLES, 0, b.count);
                }
            }
        }
        gl.bind_vertex_array(None);
        gl.depth_mask(true);
    }
}

fn upload_texture(gl: &Gl, img: &Image) -> Result<WebGlTexture, JsValue> {
    let tex = gl.create_texture().ok_or("create_texture failed")?;
    gl.bind_texture(Gl::TEXTURE_2D, Some(&tex));
    gl.tex_image_2d_with_i32_and_i32_and_i32_and_format_and_type_and_opt_u8_array(
        Gl::TEXTURE_2D,
        0,
        Gl::RGBA as i32,
        img.width as i32,
        img.height as i32,
        0,
        Gl::RGBA,
        Gl::UNSIGNED_BYTE,
        Some(&img.rgba),
    )?;
    gl.generate_mipmap(Gl::TEXTURE_2D);
    gl.tex_parameteri(Gl::TEXTURE_2D, Gl::TEXTURE_MIN_FILTER, Gl::LINEAR_MIPMAP_LINEAR as i32);
    gl.tex_parameteri(Gl::TEXTURE_2D, Gl::TEXTURE_MAG_FILTER, Gl::LINEAR as i32);
    gl.tex_parameteri(Gl::TEXTURE_2D, Gl::TEXTURE_WRAP_S, Gl::REPEAT as i32);
    gl.tex_parameteri(Gl::TEXTURE_2D, Gl::TEXTURE_WRAP_T, Gl::REPEAT as i32);
    Ok(tex)
}

fn compile(gl: &Gl, kind: u32, src: &str) -> Result<WebGlShader, JsValue> {
    let shader = gl.create_shader(kind).ok_or("create_shader failed")?;
    gl.shader_source(&shader, src);
    gl.compile_shader(&shader);
    if gl.get_shader_parameter(&shader, Gl::COMPILE_STATUS).as_bool() == Some(true) {
        Ok(shader)
    } else {
        Err(gl.get_shader_info_log(&shader).unwrap_or_default().into())
    }
}

fn link(gl: &Gl, vs: &str, fs: &str) -> Result<WebGlProgram, JsValue> {
    let program = gl.create_program().ok_or("create_program failed")?;
    gl.attach_shader(&program, &compile(gl, Gl::VERTEX_SHADER, vs)?);
    gl.attach_shader(&program, &compile(gl, Gl::FRAGMENT_SHADER, fs)?);
    gl.link_program(&program);
    if gl.get_program_parameter(&program, Gl::LINK_STATUS).as_bool() == Some(true) {
        Ok(program)
    } else {
        Err(gl.get_program_info_log(&program).unwrap_or_default().into())
    }
}
