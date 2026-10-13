//! Surface materials, indexed by an NCP polygon's `material`. Values are the
//! game's table at 0x4bd830 (27 entries); see docs/physics.md §6.

use glam::Vec3;

pub const SPARKS: u32 = 0x1;
pub const SKIDS: u32 = 0x2;
pub const CORRUGATED: u32 = 0x8;
pub const MOVING: u32 = 0x10;
pub const DUSTY: u32 = 0x20;

#[derive(Clone, Copy, Debug)]
pub struct Material {
    pub flags: u32,
    /// Multiplies static and kinetic friction.
    pub roughness: f32,
    /// Multiplies tyre/body grip.
    pub grip: f32,
    /// Multiplies body restitution.
    pub hardness: f32,
    /// Index into [`CORRUGATION`].
    pub corrugation: usize,
    /// Conveyor surface velocity (game space), used when `MOVING`.
    pub velocity: [f32; 3],
}

const fn m(flags: u32, roughness: f32, grip: f32, hardness: f32, corrugation: usize) -> Material {
    Material { flags, roughness, grip, hardness, corrugation, velocity: [0.0; 3] }
}

const fn moving(velocity: [f32; 3]) -> Material {
    Material { flags: 0x18, roughness: 1.0, grip: 1.0, hardness: 0.0, corrugation: 3, velocity }
}

pub const MATERIALS: [Material; 27] = [
    m(0x03, 1.0, 1.0, 1.0, 0),
    m(0x03, 0.9, 0.9, 0.5, 0),
    m(0x03, 0.9, 0.9, 0.5, 0),
    m(0x02, 0.8, 0.8, 0.3, 0),
    m(0x22, 0.5, 0.6, 0.0, 0),
    m(0x02, 0.7, 0.9, 0.2, 0),
    m(0x02, 1.0, 0.7, 0.0, 0),
    m(0x02, 1.0, 0.7, 0.0, 0),
    m(0x04, 1.0, 1.0, 0.0, 0),
    m(0x03, 0.7, 0.8, 0.3, 0),
    m(0x03, 0.4, 0.4, 0.2, 0),
    m(0x03, 0.8, 0.8, 0.4, 0),
    m(0x2a, 0.7, 0.5, 0.0, 3),
    m(0x0b, 0.8, 0.8, 0.4, 3),
    m(0x2b, 0.7, 0.7, 0.2, 1),
    m(0x2b, 0.9, 0.8, 0.2, 2),
    moving([-287.38, 128.74, 388.38]),
    moving([287.38, 128.74, -388.38]),
    m(0x2a, 1.0, 1.0, 0.0, 5),
    m(0x2a, 1.0, 1.0, 0.0, 6),
    m(0x2a, 1.0, 1.0, 0.0, 7),
    m(0x03, 0.45, 0.45, 0.2, 0),
    m(0x03, 0.5, 0.5, 0.2, 0),
    m(0x02, 0.8, 0.8, 0.4, 0),
    moving([0.0, 0.0, 357.78]),
    moving([-357.78, 0.0, 0.0]),
    m(0x03, 0.9, 0.9, 0.5, 0),
];

/// Bump profiles `{amplitude, wavelength x, wavelength z}` (table 0x4bdc68).
pub const CORRUGATION: [[f32; 3]; 8] = [
    [0.0, 0.0, 0.0],
    [3.0, 70.0, 70.0],
    [1.0, 40.0, 40.0],
    [1.0, 40.0, 40.0],
    [1.0, 80.0, 80.0],
    [1.0, 80.0, 80.0],
    [1.0, 80.0, 80.0],
    [1.0, 80.0, 80.0],
];

pub fn get(index: i32) -> &'static Material {
    MATERIALS.get(index as usize).unwrap_or(&MATERIALS[0])
}

impl Material {
    pub fn surface_velocity(&self) -> Vec3 {
        if self.flags & MOVING != 0 { Vec3::from(self.velocity) } else { Vec3::ZERO }
    }

    /// Extra contact depth from surface bumps at world point `p` (0x46d520).
    pub fn corrugation_depth(&self, p: Vec3) -> f32 {
        if self.flags & CORRUGATED == 0 {
            return 0.0;
        }
        let [amp, lx, lz] = CORRUGATION.get(self.corrugation).copied().unwrap_or([0.0; 3]);
        if amp == 0.0 || lx == 0.0 || lz == 0.0 {
            return 0.0;
        }
        let tau = std::f32::consts::TAU;
        amp * (tau * p.x / lx).cos() * (tau * p.z / lz).cos()
    }
}
