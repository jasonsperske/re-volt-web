//! `.w` level geometry: a list of meshes, each with culling bounds.

use super::{prm, Reader};

pub struct WorldMesh {
    pub center: [f32; 3],
    pub radius: f32,
    /// xmin, xmax, ymin, ymax, zmin, zmax
    pub bbox: [f32; 6],
    pub mesh: prm::Mesh,
}

pub struct World {
    pub meshes: Vec<WorldMesh>,
}

pub fn parse(data: &[u8]) -> Result<World, String> {
    let mut r = Reader::new(data);
    let count = r.i32()?;
    if !(0..=65536).contains(&count) {
        return Err(format!("implausible world mesh count {count}"));
    }
    let mut meshes = Vec::with_capacity(count as usize);
    for _ in 0..count {
        let center = r.vec3()?;
        let radius = r.f32()?;
        let mut bbox = [0.0; 6];
        for b in &mut bbox {
            *b = r.f32()?;
        }
        meshes.push(WorldMesh { center, radius, bbox, mesh: prm::read_mesh(&mut r)? });
    }
    // Trailing data (big culling spheres, environment colours) is not needed
    // for drawing yet.
    Ok(World { meshes })
}
