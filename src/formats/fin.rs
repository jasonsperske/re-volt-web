//! `.fin` level instances: placements of the level directory's `.prm` props.

use super::Reader;

pub struct Instance {
    /// Model name without extension, e.g. `TREE1` -> `TREE1.PRM`.
    pub name: String,
    /// Signed offset added to the model's vertex colours.
    pub color_offset: [i8; 3],
    pub env_rgb: u32,
    pub priority: u8,
    pub flags: u8,
    pub lod_bias: f32,
    pub pos: [f32; 3],
    /// Rotation rows: right, up, look. A point transforms as
    /// `x*right + y*up + z*look + pos`.
    pub rot: [[f32; 3]; 3],
}

pub fn parse(data: &[u8]) -> Result<Vec<Instance>, String> {
    let mut r = Reader::new(data);
    let count = r.i32()?;
    if count < 0 || count as usize * 72 > r.remaining() {
        return Err(format!("bad instance count {count}"));
    }
    let mut out = Vec::with_capacity(count as usize);
    for _ in 0..count {
        let raw_name = r.bytes(9)?;
        let end = raw_name.iter().position(|&b| b == 0).unwrap_or(9);
        let name = String::from_utf8_lossy(&raw_name[..end]).to_ascii_uppercase();
        let color_offset = [r.i8()?, r.i8()?, r.i8()?];
        let env_rgb = r.u32()?;
        let priority = r.u8()?;
        let flags = r.u8()?;
        r.skip(2)?;
        let lod_bias = r.f32()?;
        let pos = r.vec3()?;
        let rot = [r.vec3()?, r.vec3()?, r.vec3()?];
        out.push(Instance { name, color_offset, env_rgb, priority, flags, lod_bias, pos, rot });
    }
    Ok(out)
}
