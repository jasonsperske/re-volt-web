//! Loads levels and cars from the disc into parsed, render-ready assets.

use std::collections::BTreeMap;

use crate::disc::{ByteSource, Disc};
use crate::formats::{bmp, fan, fin, fob, hul, inf, ncp, pan, params, prm, taz, world};

/// The installed game lives under this directory on the CD.
const GAME_ROOT: &str = "REVOLT";

pub struct GameData<S> {
    disc: Disc<S>,
}

pub struct Entry {
    /// Directory name, used as the identifier (`NHOOD1`, `ADEON`).
    pub id: String,
    /// Display name from the level/car files.
    pub name: String,
}

pub struct LevelAssets {
    pub id: String,
    pub info: inf::LevelInfo,
    pub world: world::World,
    pub instances: Vec<(fin::Instance, std::rc::Rc<prm::Mesh>)>,
    /// Keyed by texture page index used in polygons.
    pub textures: BTreeMap<i16, bmp::Image>,
    /// Track collision (`<level>.ncp`).
    pub collision: Vec<ncp::CollPoly>,
    /// Prop collision by instance model name, in the model's own space.
    pub instance_collision: BTreeMap<String, Vec<ncp::CollPoly>>,
    /// Track zones and position nodes (empty for arenas).
    pub zones: Vec<taz::Zone>,
    pub pos_nodes: pan::PosNodes,
    /// AI racing line (empty for arenas).
    pub ai_nodes: fan::AiNodes,
    /// Placed level objects (pickup spawns and more).
    pub objects: Vec<fob::LevelObject>,
}

pub struct CarPart {
    pub mesh: prm::Mesh,
    /// Position relative to the car's model centre (game space).
    pub offset: [f32; 3],
}

pub struct CarAssets {
    pub id: String,
    pub params: params::CarParams,
    /// Body first (when present), then the wheels in parameter order.
    pub parts: Vec<CarPart>,
    /// Index into `params.wheels` for each wheel part, `None` for the body.
    pub part_wheel: Vec<Option<usize>>,
    pub texture: Option<bmp::Image>,
    /// From the `COLL` file; `None` if missing or unreadable.
    pub hull: Option<hul::Hull>,
}

impl CarAssets {
    /// Bounds of the body mesh in car space (game axes).
    pub fn body_bounds(&self) -> (glam::Vec3, glam::Vec3) {
        let body = self.parts.iter().zip(&self.part_wheel).find(|(_, w)| w.is_none()).map(|(p, _)| p);
        let mut lo = glam::Vec3::splat(f32::MAX);
        let mut hi = glam::Vec3::splat(f32::MIN);
        for v in body.iter().flat_map(|p| p.mesh.verts.iter()) {
            let p = glam::Vec3::from(v.pos) + glam::Vec3::from(body.unwrap().offset);
            lo = lo.min(p);
            hi = hi.max(p);
        }
        if lo.x > hi.x {
            return (glam::Vec3::new(-20.0, -10.0, -35.0), glam::Vec3::new(20.0, 5.0, 35.0));
        }
        (lo, hi)
    }
}

impl<S: ByteSource> GameData<S> {
    pub async fn open(source: S) -> Result<Self, String> {
        let disc = Disc::open(source).await?;
        if !disc.exists(&format!("{GAME_ROOT}/LEVELS/NHOOD1/NHOOD1.W")) {
            return Err("this does not look like the Re-Volt PC disc (no REVOLT/LEVELS)".into());
        }
        Ok(GameData { disc })
    }

    pub fn disc(&self) -> &Disc<S> {
        &self.disc
    }

    /// Reads a file by its in-game relative path, e.g. `cars\adeon\body.prm`.
    pub async fn read(&self, game_path: &str) -> Result<Vec<u8>, String> {
        self.disc.read_file(&format!("{GAME_ROOT}/{game_path}")).await
    }

    fn exists(&self, game_path: &str) -> bool {
        self.disc.exists(&format!("{GAME_ROOT}/{game_path}"))
    }

    /// Levels with drawable geometry (skips FRONTEND, the menu scene).
    pub async fn list_levels(&self) -> Result<Vec<Entry>, String> {
        let mut out = Vec::new();
        for id in self.disc.list_dir(&format!("{GAME_ROOT}/LEVELS")) {
            let base = format!("LEVELS/{id}/{id}");
            if id == "FRONTEND" || !self.exists(&format!("{base}.W")) || !self.exists(&format!("{base}.INF")) {
                continue;
            }
            let info = inf::parse(&self.read(&format!("{base}.INF")).await?);
            let name = if info.name.is_empty() { id.clone() } else { info.name };
            out.push(Entry { id, name });
        }
        Ok(out)
    }

    pub async fn list_cars(&self) -> Result<Vec<Entry>, String> {
        let mut out = Vec::new();
        for id in self.disc.list_dir(&format!("{GAME_ROOT}/CARS")) {
            let path = format!("CARS/{id}/PARAMETERS.TXT");
            if !self.exists(&path) {
                continue;
            }
            let p = params::parse(&self.read(&path).await?);
            let name = if p.name.is_empty() { id.clone() } else { p.name };
            out.push(Entry { id, name });
        }
        out.sort_by(|a, b| a.name.to_ascii_lowercase().cmp(&b.name.to_ascii_lowercase()));
        Ok(out)
    }

    pub async fn load_level(&self, id: &str) -> Result<LevelAssets, String> {
        let base = format!("LEVELS/{id}/{id}");
        let info = inf::parse(&self.read(&format!("{base}.INF")).await?);
        let world = world::parse(&self.read(&format!("{base}.W")).await?)?;

        let collision = ncp::parse(&self.read(&format!("{base}.NCP")).await?)?;
        let mut instance_collision = BTreeMap::new();
        let mut instances = Vec::new();
        if self.exists(&format!("{base}.FIN")) {
            let mut models: BTreeMap<String, Option<std::rc::Rc<prm::Mesh>>> = BTreeMap::new();
            for inst in fin::parse(&self.read(&format!("{base}.FIN")).await?)? {
                if !models.contains_key(&inst.name) {
                    let path = format!("LEVELS/{id}/{}.PRM", inst.name);
                    let mesh = if self.exists(&path) {
                        prm::parse(&self.read(&path).await?).ok().map(std::rc::Rc::new)
                    } else {
                        None
                    };
                    models.insert(inst.name.clone(), mesh);
                    let ncp_path = format!("LEVELS/{id}/{}.NCP", inst.name);
                    if self.exists(&ncp_path) {
                        instance_collision.insert(inst.name.clone(), ncp::parse(&self.read(&ncp_path).await?)?);
                    }
                }
                if let Some(Some(mesh)) = models.get(&inst.name) {
                    let mesh = mesh.clone();
                    instances.push((inst, mesh));
                }
            }
        }

        let mut pages: Vec<i16> = world
            .meshes
            .iter()
            .flat_map(|m| m.mesh.polys.iter())
            .chain(instances.iter().flat_map(|(_, m)| m.polys.iter()))
            .map(|p| p.texture)
            .filter(|&t| t >= 0)
            .collect();
        pages.sort_unstable();
        pages.dedup();
        let mut textures = BTreeMap::new();
        for page in pages {
            // Page 0 is <level>A.BMP, page 1 <level>B.BMP, ...
            let Some(letter) = char::from_u32('A' as u32 + page as u32).filter(char::is_ascii_uppercase) else {
                continue;
            };
            let path = format!("{base}{letter}.BMP");
            if self.exists(&path) {
                if let Ok(img) = bmp::decode(&self.read(&path).await?) {
                    textures.insert(page, img);
                }
            }
        }
        let zones = match self.read(&format!("{base}.TAZ")).await {
            Ok(d) => taz::parse(&d).unwrap_or_default(),
            Err(_) => Vec::new(),
        };
        let pos_nodes = match self.read(&format!("{base}.PAN")).await {
            Ok(d) => pan::parse(&d).unwrap_or_default(),
            Err(_) => pan::PosNodes::default(),
        };
        let ai_nodes = match self.read(&format!("{base}.FAN")).await {
            Ok(d) => fan::parse(&d).unwrap_or_default(),
            Err(_) => fan::AiNodes::default(),
        };
        let objects = match self.read(&format!("{base}.FOB")).await {
            Ok(d) => fob::parse(&d).unwrap_or_default(),
            Err(_) => Vec::new(),
        };
        Ok(LevelAssets { id: id.to_string(), info, world, instances, textures, collision, instance_collision, zones, pos_nodes, ai_nodes, objects })
    }

    pub async fn load_car(&self, id: &str) -> Result<CarAssets, String> {
        let params = params::parse(&self.read(&format!("CARS/{id}/PARAMETERS.TXT")).await?);
        let mut parts = Vec::new();
        let mut part_wheel = Vec::new();
        if let Some(mesh) = self.load_model(&params, params.body.model).await {
            parts.push(CarPart { mesh, offset: params.body.offset });
            part_wheel.push(None);
        }
        // Wheel parts carry their slot (0..3), matching the physics.
        for (n, w) in params.wheels.iter().enumerate().filter(|(_, w)| w.present) {
            if let Some(mesh) = self.load_model(&params, w.model).await {
                parts.push(CarPart { mesh, offset: w.center() });
                part_wheel.push(Some(n));
            }
        }
        let texture = match &params.tpage {
            Some(p) if self.exists(p) => bmp::decode(&self.read(p).await?).ok(),
            _ => None,
        };
        let hull = match &params.coll {
            Some(p) if self.exists(p) => hul::parse(&self.read(p).await?).ok(),
            _ => None,
        };
        Ok(CarAssets { id: id.to_string(), params, parts, part_wheel, texture, hull })
    }

    /// Loads a standalone model (e.g. `MODELS/PICKUP.M`).
    pub async fn load_mesh(&self, path: &str) -> Result<prm::Mesh, String> {
        prm::parse(&self.read(path).await?)
    }

    async fn load_model(&self, params: &params::CarParams, slot: Option<usize>) -> Option<prm::Mesh> {
        let path = params.models.get(slot?)?.as_ref()?;
        // `.m` models (trolley, aerial) use the same layout as `.prm`.
        let lower = path.to_ascii_lowercase();
        if !(lower.ends_with(".prm") || lower.ends_with(".m")) || !self.exists(path) {
            return None;
        }
        prm::parse(&self.read(path).await.ok()?).ok()
    }
}
