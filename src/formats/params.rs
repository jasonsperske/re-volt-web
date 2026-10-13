//! Car `Parameters.txt`: model assembly plus the handling values used by
//! the physics.

use super::{text, tokenize};

pub const MODEL_SLOTS: usize = 19;

#[derive(Clone, Debug, Default)]
pub struct Body {
    pub model: Option<usize>,
    pub offset: [f32; 3],
    pub mass: f32,
    /// Diagonal of the inertia tensor (the files only use diagonal values).
    pub inertia: [f32; 3],
    /// Caps friction impulses (not the gravity acceleration).
    pub gravity: f32,
    /// Restitution for body contacts.
    pub hardness: f32,
    pub resistance: f32,
    pub ang_res: f32,
    /// Angular resistance multiplier while airborne.
    pub res_mod: f32,
    pub grip: f32,
    pub static_friction: f32,
    pub kinetic_friction: f32,
}

#[derive(Clone, Debug, Default)]
pub struct Wheel {
    pub model: Option<usize>,
    /// Suspension mount point relative to the body.
    pub offset1: [f32; 3],
    /// Wheel centre relative to `offset1`.
    pub offset2: [f32; 3],
    pub present: bool,
    pub powered: bool,
    pub turnable: bool,
    pub steer_ratio: f32,
    pub engine_ratio: f32,
    pub radius: f32,
    pub mass: f32,
    pub gravity: f32,
    /// Suspension travel.
    pub max_pos: f32,
    pub axle_friction: f32,
    pub grip: f32,
    pub static_friction: f32,
    pub kinetic_friction: f32,
}

impl Wheel {
    pub fn center(&self) -> [f32; 3] {
        [self.offset1[0] + self.offset2[0], self.offset1[1] + self.offset2[1], self.offset1[2] + self.offset2[2]]
    }
}

#[derive(Clone, Debug, Default)]
pub struct Spring {
    pub stiffness: f32,
    pub damping: f32,
    pub restitution: f32,
}

#[derive(Clone, Debug, Default)]
pub struct CarParams {
    pub name: String,
    /// Engine type: 0 electric, 1 glow, 2 other.
    pub class: i32,
    /// Game-relative model paths; `None` for "NONE".
    pub models: Vec<Option<String>>,
    pub tpage: Option<String>,
    /// Collision hull path (`COLL`).
    pub coll: Option<String>,
    pub steer_rate: f32,
    pub steer_mod: f32,
    pub engine_rate: f32,
    pub top_speed: f32,
    pub down_force_mod: f32,
    pub com: [f32; 3],
    /// Where projectiles are launched from, car space (`Weapon`).
    pub weapon: [f32; 3],
    pub body: Body,
    pub wheels: [Wheel; 4],
    pub springs: [Spring; 4],
}

#[derive(Clone, Copy, PartialEq)]
enum Block {
    /// The bare `{` wrapping the whole file; behaves like top level.
    File,
    Body,
    Wheel(usize),
    Spring(usize),
    Other,
}

pub fn parse(data: &[u8]) -> CarParams {
    let mut car = CarParams { models: vec![None; MODEL_SLOTS], ..Default::default() };
    let mut stack: Vec<Block> = Vec::new();
    // Inertia is written as three rows over three lines.
    let mut inertia_row: Option<usize> = None;
    for line in text(data).lines() {
        let mut t = tokenize(line);
        if t.is_empty() {
            continue;
        }
        let opens = t.last().is_some_and(|s| s == "{");
        if opens {
            t.pop();
        }
        if t.first().is_some_and(|s| s == "}") {
            stack.pop();
            continue;
        }
        let key = t.first().map(|s| s.to_ascii_uppercase()).unwrap_or_default();
        let index = || t.get(1).and_then(|s| s.parse::<usize>().ok()).filter(|&n| n < 4);
        if opens {
            stack.push(match (key.as_str(), index()) {
                ("", _) => Block::File,
                ("BODY", _) => Block::Body,
                ("WHEEL", Some(n)) => Block::Wheel(n),
                ("SPRING", Some(n)) => Block::Spring(n),
                _ => Block::Other,
            });
            continue;
        }
        let num = |i: usize| t.get(i).and_then(|s| s.parse::<f32>().ok()).unwrap_or(0.0);
        let v3 = |i: usize| [num(i), num(i + 1), num(i + 2)];
        let flag = || t.get(1).is_some_and(|s| s.eq_ignore_ascii_case("TRUE"));
        let model_num = || t.get(1).and_then(|s| s.parse::<i32>().ok()).filter(|&n| n >= 0).map(|n| n as usize);
        let block = stack.last().copied().unwrap_or(Block::File);

        // A continuation row of the inertia matrix has no key, just numbers.
        if let Some(row) = inertia_row {
            if key.parse::<f32>().is_ok() {
                car.body.inertia[row] = num(row);
                inertia_row = (row < 2).then_some(row + 1);
                continue;
            }
            inertia_row = None;
        }

        match block {
            Block::File => match key.as_str() {
                "NAME" => car.name = t.get(1).cloned().unwrap_or_default(),
                "CLASS" => car.class = num(1) as i32,
                "MODEL" => {
                    if let (Some(slot), Some(path)) = (t.get(1).and_then(|s| s.parse::<usize>().ok()), t.get(2)) {
                        if slot < MODEL_SLOTS && !path.eq_ignore_ascii_case("NONE") {
                            car.models[slot] = Some(path.clone());
                        }
                    }
                }
                "TPAGE" => car.tpage = t.get(1).filter(|p| !p.eq_ignore_ascii_case("NONE")).cloned(),
                "COLL" => car.coll = t.get(1).filter(|p| !p.eq_ignore_ascii_case("NONE")).cloned(),
                "STEERRATE" => car.steer_rate = num(1),
                "STEERMOD" => car.steer_mod = num(1),
                "ENGINERATE" => car.engine_rate = num(1),
                "TOPSPEED" => car.top_speed = num(1),
                "DOWNFORCEMOD" => car.down_force_mod = num(1),
                "COM" => car.com = v3(1),
                "WEAPON" => car.weapon = v3(1),
                _ => {}
            },
            Block::Body => {
                let b = &mut car.body;
                match key.as_str() {
                    "MODELNUM" => b.model = model_num(),
                    "OFFSET" => b.offset = v3(1),
                    "MASS" => b.mass = num(1),
                    "INERTIA" => {
                        // Row 0 is on this line, after the key.
                        b.inertia[0] = num(1);
                        inertia_row = Some(1);
                    }
                    "GRAVITY" => b.gravity = num(1),
                    "HARDNESS" => b.hardness = num(1),
                    "RESISTANCE" => b.resistance = num(1),
                    "ANGRES" => b.ang_res = num(1),
                    "RESMOD" => b.res_mod = num(1),
                    "GRIP" => b.grip = num(1),
                    "STATICFRICTION" => b.static_friction = num(1),
                    "KINETICFRICTION" => b.kinetic_friction = num(1),
                    _ => {}
                }
            }
            Block::Wheel(n) => {
                let w = &mut car.wheels[n];
                match key.as_str() {
                    "MODELNUM" => w.model = model_num(),
                    "OFFSET1" => w.offset1 = v3(1),
                    "OFFSET2" => w.offset2 = v3(1),
                    "ISPRESENT" => w.present = flag(),
                    "ISPOWERED" => w.powered = flag(),
                    "ISTURNABLE" => w.turnable = flag(),
                    "STEERRATIO" => w.steer_ratio = num(1),
                    "ENGINERATIO" => w.engine_ratio = num(1),
                    "RADIUS" => w.radius = num(1),
                    "MASS" => w.mass = num(1),
                    "GRAVITY" => w.gravity = num(1),
                    "MAXPOS" => w.max_pos = num(1),
                    "AXLEFRICTION" => w.axle_friction = num(1),
                    "GRIP" => w.grip = num(1),
                    "STATICFRICTION" => w.static_friction = num(1),
                    "KINETICFRICTION" => w.kinetic_friction = num(1),
                    _ => {}
                }
            }
            Block::Spring(n) => {
                let s = &mut car.springs[n];
                match key.as_str() {
                    "STIFFNESS" => s.stiffness = num(1),
                    "DAMPING" => s.damping = num(1),
                    "RESTITUTION" => s.restitution = num(1),
                    _ => {}
                }
            }
            Block::Other => {}
        }
    }
    car
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_nested_blocks_and_inertia_rows() {
        let src = b"{\nName \"Test\"\nTopSpeed 36.5\nBODY {\nMass 1.2\nInertia 1000 0 0\n 0 1700 0\n 0 0 560\nResistance 0.001\n}\n\
WHEEL 1 { ; c\nOffset1 23.5 -3 39\nIsPowered TRUE\nRadius 10\n}\nSPRING 2 {\nStiffness 1000\nDamping 15\n}\nAI {\nAI {\nMass 99\n}\n}\n}\n";
        let car = parse(src);
        assert_eq!(car.name, "Test");
        assert_eq!(car.top_speed, 36.5);
        assert_eq!(car.body.mass, 1.2);
        assert_eq!(car.body.inertia, [1000.0, 1700.0, 560.0]);
        assert_eq!(car.body.resistance, 0.001);
        assert_eq!(car.wheels[1].offset1, [23.5, -3.0, 39.0]);
        assert!(car.wheels[1].powered);
        assert_eq!(car.springs[2].stiffness, 1000.0);
        assert_eq!(car.springs[2].damping, 15.0);
    }
}
