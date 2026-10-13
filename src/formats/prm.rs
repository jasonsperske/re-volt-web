//! `.prm` single meshes (cars, level props) and the mesh records embedded in `.w`.

use super::Reader;

pub const POLY_QUAD: u16 = 0x001;
pub const POLY_DOUBLE_SIDED: u16 = 0x002;
pub const POLY_SEMI_TRANSPARENT: u16 = 0x004;
pub const POLY_ADDITIVE: u16 = 0x100;
/// In `.w` files: polygon has an entry in the trailing environment colour list.
pub const POLY_WORLD_ENV: u16 = 0x800;

#[derive(Clone, Debug)]
pub struct Poly {
    pub flags: u16,
    /// Texture page index, or -1 for untextured.
    pub texture: i16,
    pub indices: [u16; 4],
    /// Per-vertex colour, 0xAARRGGBB.
    pub colors: [u32; 4],
    pub uvs: [[f32; 2]; 4],
}

impl Poly {
    pub fn vertex_count(&self) -> usize {
        if self.flags & POLY_QUAD != 0 { 4 } else { 3 }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Vertex {
    pub pos: [f32; 3],
    pub normal: [f32; 3],
}

#[derive(Clone, Debug, Default)]
pub struct Mesh {
    pub polys: Vec<Poly>,
    pub verts: Vec<Vertex>,
}

pub fn read_mesh(r: &mut Reader) -> Result<Mesh, String> {
    let poly_count = r.u16()? as usize;
    let vert_count = r.u16()? as usize;
    let mut polys = Vec::with_capacity(poly_count);
    for _ in 0..poly_count {
        let flags = r.u16()?;
        let texture = r.i16()?;
        let indices = [r.u16()?, r.u16()?, r.u16()?, r.u16()?];
        let colors = [r.u32()?, r.u32()?, r.u32()?, r.u32()?];
        let mut uvs = [[0.0; 2]; 4];
        for uv in &mut uvs {
            *uv = [r.f32()?, r.f32()?];
        }
        polys.push(Poly { flags, texture, indices, colors, uvs });
    }
    let mut verts = Vec::with_capacity(vert_count);
    for _ in 0..vert_count {
        verts.push(Vertex { pos: r.vec3()?, normal: r.vec3()? });
    }
    for p in &polys {
        if p.indices[..p.vertex_count()].iter().any(|&i| i as usize >= vert_count) {
            return Err("polygon references a vertex out of range".into());
        }
    }
    Ok(Mesh { polys, verts })
}

/// Parses a `.prm` file. Some files (car wheels) append lower-detail meshes
/// after the first; only the full-detail mesh is returned.
pub fn parse(data: &[u8]) -> Result<Mesh, String> {
    read_mesh(&mut Reader::new(data))
}
