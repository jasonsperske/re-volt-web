//! `.inf` level settings (name, start grid, fog, draw distance).

use super::{text, tokenize};

#[derive(Clone, Debug)]
pub struct LevelInfo {
    pub name: String,
    pub start_pos: [f32; 3],
    /// Fraction of a full turn (0..1) about the vertical axis.
    pub start_rot: f32,
    pub far_clip: f32,
    pub fog_start: f32,
    pub fog_color: [u8; 3],
    /// CD audio tracks this level plays music from.
    pub redbook: Option<(u32, u32)>,
}

impl Default for LevelInfo {
    fn default() -> Self {
        LevelInfo {
            name: String::new(),
            start_pos: [0.0; 3],
            start_rot: 0.0,
            far_clip: 20000.0,
            fog_start: 19000.0,
            fog_color: [0, 0, 0],
            redbook: None,
        }
    }
}

pub fn parse(data: &[u8]) -> LevelInfo {
    let mut info = LevelInfo::default();
    for line in text(data).lines() {
        let t = tokenize(line);
        let Some(key) = t.first() else { continue };
        let f = |i: usize| t.get(i).and_then(|s| s.parse::<f32>().ok());
        match key.to_ascii_uppercase().as_str() {
            "NAME" => info.name = t.get(1).cloned().unwrap_or_default(),
            "STARTPOS" => {
                if let (Some(x), Some(y), Some(z)) = (f(1), f(2), f(3)) {
                    info.start_pos = [x, y, z];
                }
            }
            "STARTROT" => info.start_rot = f(1).unwrap_or(0.0),
            "FARCLIP" => info.far_clip = f(1).unwrap_or(info.far_clip),
            "FOGSTART" => info.fog_start = f(1).unwrap_or(info.fog_start),
            "FOGCOLOR" => {
                if let (Some(r), Some(g), Some(b)) = (f(1), f(2), f(3)) {
                    info.fog_color = [r as u8, g as u8, b as u8];
                }
            }
            "REDBOOK" => {
                if let (Some(a), Some(b)) = (f(1), f(2)) {
                    info.redbook = Some((a as u32, b as u32));
                }
            }
            _ => {}
        }
    }
    info
}
