//! Workbench tuning: a player's changes to a car, applied to its
//! `PARAMETERS.TXT` values before the physics setup is built. Every
//! adjustment is a multiplier of the stock value, so 1.0 is stock.

use crate::formats::params::CarParams;

/// The adjustable attributes, in workbench order: (key, what it scales).
pub const KEYS: [&str; 9] = ["mass", "stiffness", "damping", "travel", "grip", "power", "top_speed", "steering", "downforce"];

#[derive(Clone, Debug, PartialEq)]
pub struct Tuning {
    /// Replaces the car's name when set.
    pub name: Option<String>,
    /// Multipliers in [`KEYS`] order.
    pub scale: [f32; KEYS.len()],
}

impl Default for Tuning {
    fn default() -> Self {
        Tuning { name: None, scale: [1.0; KEYS.len()] }
    }
}

impl Tuning {
    /// The multiplier for `key` (1.0 for unknown keys).
    pub fn get(&self, key: &str) -> f32 {
        KEYS.iter().position(|k| *k == key).map_or(1.0, |i| self.scale[i])
    }

    /// Sets a multiplier, kept within a range the physics copes with.
    pub fn set(&mut self, key: &str, value: f32) {
        if let Some(i) = KEYS.iter().position(|k| *k == key) {
            self.scale[i] = if value.is_finite() { value.clamp(0.25, 4.0) } else { 1.0 };
        }
    }

    pub fn is_stock(&self) -> bool {
        *self == Tuning::default()
    }

    pub fn apply(&self, p: &mut CarParams) {
        if let Some(name) = self.name.as_ref().map(|n| n.trim()).filter(|n| !n.is_empty()) {
            p.name = name.chars().take(24).collect();
        }
        // Heavier cars are proportionally harder to spin, as if built denser.
        let mass = self.get("mass");
        p.body.mass *= mass;
        for v in &mut p.body.inertia {
            *v *= mass;
        }
        for s in &mut p.springs {
            s.stiffness *= self.get("stiffness");
            s.damping *= self.get("damping");
        }
        for w in &mut p.wheels {
            w.max_pos *= self.get("travel");
            w.grip *= self.get("grip");
            w.static_friction *= self.get("grip");
            w.kinetic_friction *= self.get("grip");
            w.engine_ratio *= self.get("power");
        }
        p.top_speed *= self.get("top_speed");
        p.steer_rate *= self.get("steering");
        p.down_force_mod *= self.get("downforce");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::formats::params;

    #[test]
    fn stock_tuning_changes_nothing_and_scales_apply() {
        let src = b"{\nName \"Test\"\nTopSpeed 30\nSteerRate 3\nBODY {\nMass 1.2\nInertia 1000 0 0\n 0 1700 0\n 0 0 560\n}\n\
WHEEL 0 {\nMaxPos 8\nGrip 0.015\nStaticFriction 1.5\nKineticFriction 1.4\nEngineRatio 6500\n}\nSPRING 0 {\nStiffness 300\nDamping 7\n}\n}\n";
        let stock = params::parse(src);
        let mut same = stock.clone();
        Tuning::default().apply(&mut same);
        assert_eq!(format!("{same:?}"), format!("{stock:?}"));

        let mut t = Tuning { name: Some("  Slugger ".into()), ..Default::default() };
        t.set("mass", 2.0);
        t.set("stiffness", 1.5);
        t.set("top_speed", 0.5);
        t.set("grip", 9.0); // clamped
        let mut p = stock.clone();
        t.apply(&mut p);
        assert_eq!(p.name, "Slugger");
        assert_eq!(p.body.mass, 2.4);
        assert_eq!(p.body.inertia, [2000.0, 3400.0, 1120.0]);
        assert_eq!(p.springs[0].stiffness, 450.0);
        assert_eq!(p.springs[0].damping, 7.0);
        assert_eq!(p.top_speed, 15.0);
        assert_eq!(p.wheels[0].grip, 0.015 * 4.0);
        assert!(!t.is_stock());
    }
}
