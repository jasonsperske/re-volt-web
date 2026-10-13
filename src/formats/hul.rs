//! Car `.hul` collision hulls: one or more convex hulls plus a set of
//! spheres. Layout verified against every car on the disc (33 parse to the
//! exact byte; MOUSE has 644 trailing bytes after a valid hull, ignored).

use super::Reader;

#[derive(Clone, Debug)]
pub struct ConvexHull {
    /// xmin, xmax, ymin, ymax, zmin, zmax
    pub bbox: [f32; 6],
    pub offset: [f32; 3],
    pub verts: Vec<[f32; 3]>,
    pub edges: Vec<[u16; 2]>,
    /// Face planes: normal xyz, d.
    pub planes: Vec<[f32; 4]>,
}

#[derive(Clone, Debug, Default)]
pub struct Hull {
    pub convex: Vec<ConvexHull>,
    /// Collision spheres in car space: centre xyz, radius.
    pub spheres: Vec<[f32; 4]>,
}

pub fn parse(data: &[u8]) -> Result<Hull, String> {
    if data.is_empty() {
        return Err("empty hull file".into());
    }
    let mut r = Reader::new(data);
    let hull_count = r.u16()?;
    let mut convex = Vec::with_capacity(hull_count as usize);
    for _ in 0..hull_count {
        let (nv, ne, nf) = (r.u16()? as usize, r.u16()? as usize, r.u16()? as usize);
        let mut bbox = [0.0; 6];
        for b in &mut bbox {
            *b = r.f32()?;
        }
        let offset = r.vec3()?;
        let verts = (0..nv).map(|_| r.vec3()).collect::<Result<_, _>>()?;
        let edges = (0..ne).map(|_| Ok([r.u16()?, r.u16()?])).collect::<Result<_, String>>()?;
        let planes = (0..nf).map(|_| Ok([r.f32()?, r.f32()?, r.f32()?, r.f32()?])).collect::<Result<_, String>>()?;
        convex.push(ConvexHull { bbox, offset, verts, edges, planes });
    }
    let sphere_count = r.u16()? as usize;
    let spheres = (0..sphere_count)
        .map(|_| Ok([r.f32()?, r.f32()?, r.f32()?, r.f32()?]))
        .collect::<Result<_, String>>()?;
    Ok(Hull { convex, spheres })
}
