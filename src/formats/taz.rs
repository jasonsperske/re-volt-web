//! `.taz` track zones: oriented boxes numbered in racing order, used for lap
//! counting. Layout verified on every level (4 + 64·count bytes).

use super::Reader;

#[derive(Clone, Debug)]
pub struct Zone {
    pub id: i32,
    pub pos: [f32; 3],
    /// Rows: the box's local X, Y, Z axes in world space.
    pub rot: [[f32; 3]; 3],
    /// Half-extents along the local axes.
    pub size: [f32; 3],
}

impl Zone {
    pub fn contains(&self, p: [f32; 3]) -> bool {
        let d = [p[0] - self.pos[0], p[1] - self.pos[1], p[2] - self.pos[2]];
        (0..3).all(|i| {
            let along = d[0] * self.rot[i][0] + d[1] * self.rot[i][1] + d[2] * self.rot[i][2];
            along.abs() <= self.size[i]
        })
    }
}

pub fn parse(data: &[u8]) -> Result<Vec<Zone>, String> {
    let mut r = Reader::new(data);
    let count = r.i32()?;
    if count < 0 || count as usize * 64 > r.remaining() {
        return Err(format!("bad zone count {count}"));
    }
    (0..count)
        .map(|_| Ok(Zone { id: r.i32()?, pos: r.vec3()?, rot: [r.vec3()?, r.vec3()?, r.vec3()?], size: r.vec3()? }))
        .collect()
}
