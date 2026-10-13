//! Turns parsed meshes into GPU-ready triangle batches.
//!
//! Re-Volt's world space is right-handed with Y pointing down and Z forward.
//! Everything is converted to OpenGL convention (Y up, -Z forward) here by a
//! 180° rotation about X, which keeps handedness, so the original clockwise
//! front-face winding is preserved.

use std::collections::BTreeMap;

use glam::{Mat3, Vec3};

use crate::formats::prm::{self, Mesh};

pub fn to_gl(p: Vec3) -> Vec3 {
    Vec3::new(p.x, -p.y, -p.z)
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct GpuVertex {
    pub pos: [f32; 3],
    pub uv: [f32; 2],
    pub color: [u8; 4],
}

pub const VERTEX_SIZE: usize = std::mem::size_of::<GpuVertex>();

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Blend {
    Opaque,
    Alpha,
    Additive,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BatchKey {
    /// Texture slot in the owning scene, or `None` for untextured.
    pub texture: Option<i16>,
    pub blend: Blend,
    pub double_sided: bool,
}

/// Placement of a mesh in game space: `p' = rot * p + pos`.
#[derive(Clone, Copy)]
pub struct Placement {
    pub rot: Mat3,
    pub pos: Vec3,
    pub color_offset: [i8; 3],
}

impl Placement {
    pub fn at(pos: [f32; 3]) -> Self {
        Placement { rot: Mat3::IDENTITY, pos: Vec3::from(pos), color_offset: [0; 3] }
    }
}

#[derive(Default)]
pub struct SceneBuilder {
    pub batches: BTreeMap<BatchKey, Vec<GpuVertex>>,
}

impl SceneBuilder {
    /// `texture_slot` maps a polygon's texture page to a scene texture slot.
    pub fn add_mesh(&mut self, mesh: &Mesh, place: &Placement, texture_slot: impl Fn(i16) -> Option<i16>) {
        let verts: Vec<Vec3> = mesh.verts.iter().map(|v| to_gl(place.rot * Vec3::from(v.pos) + place.pos)).collect();
        for poly in &mesh.polys {
            let blend = if poly.flags & prm::POLY_ADDITIVE != 0 {
                Blend::Additive
            } else if poly.flags & prm::POLY_SEMI_TRANSPARENT != 0 {
                Blend::Alpha
            } else {
                Blend::Opaque
            };
            let key = BatchKey {
                texture: if poly.texture >= 0 { texture_slot(poly.texture) } else { None },
                blend,
                double_sided: poly.flags & prm::POLY_DOUBLE_SIDED != 0,
            };
            let out = self.batches.entry(key).or_default();
            let corner = |i: usize| {
                let [b, g, r, a] = poly.colors[i].to_le_bytes();
                let shade = |c: u8, off: i8| (c as i16 + off as i16).clamp(0, 255) as u8;
                GpuVertex {
                    pos: verts[poly.indices[i] as usize].to_array(),
                    uv: poly.uvs[i],
                    color: [
                        shade(r, place.color_offset[0]),
                        shade(g, place.color_offset[1]),
                        shade(b, place.color_offset[2]),
                        // Vertex alpha is only meaningful on translucent polys.
                        if blend == Blend::Opaque { 255 } else { a },
                    ],
                }
            };
            out.extend([corner(0), corner(1), corner(2)]);
            if poly.vertex_count() == 4 {
                out.extend([corner(0), corner(2), corner(3)]);
            }
        }
    }

    pub fn triangle_count(&self) -> usize {
        self.batches.values().map(|v| v.len() / 3).sum()
    }
}

/// Converts a `.fin` row-vector rotation (right, up, look rows) into a
/// column-major matrix acting on column vectors.
pub fn instance_rotation(rows: &[[f32; 3]; 3]) -> Mat3 {
    Mat3::from_cols(Vec3::from(rows[0]), Vec3::from(rows[1]), Vec3::from(rows[2]))
}

pub fn level_scene(level: &crate::assets::LevelAssets) -> SceneBuilder {
    let mut b = SceneBuilder::default();
    let slot = |t: i16| Some(t);
    let origin = Placement::at([0.0; 3]);
    for m in &level.world.meshes {
        b.add_mesh(&m.mesh, &origin, slot);
    }
    for (inst, mesh) in &level.instances {
        let place = Placement { rot: instance_rotation(&inst.rot), pos: Vec3::from(inst.pos), color_offset: inst.color_offset };
        b.add_mesh(mesh, &place, slot);
    }
    b
}

/// One drawable piece of a car, in its own local GL space; the body,
/// wheels and suspension parts move independently at runtime.
pub struct CarPartScene {
    pub builder: SceneBuilder,
    pub kind: crate::assets::PartKind,
    /// Rest offset from the car origin (game space).
    pub offset: [f32; 3],
}

/// Every textured car polygon uses the car's single texture page, which the
/// renderer stores in slot 0.
pub fn car_parts(car: &crate::assets::CarAssets) -> Vec<CarPartScene> {
    car.parts
        .iter()
        .zip(&car.part_kind)
        .map(|(part, &kind)| {
            let mut builder = SceneBuilder::default();
            builder.add_mesh(&part.mesh, &Placement::at([0.0; 3]), |_| Some(0));
            CarPartScene { builder, kind, offset: part.offset }
        })
        .collect()
}

/// Converts a game-space rigid transform into a GL-space model matrix for
/// geometry already converted with [`to_gl`].
pub fn gl_model(pos: Vec3, rot: glam::Quat) -> glam::Mat4 {
    let flip = glam::Quat::from_rotation_x(std::f32::consts::PI);
    glam::Mat4::from_rotation_translation(flip * rot * flip.inverse(), to_gl(pos))
}
