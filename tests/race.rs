//! Lap counting and standings on real tracks: a point is moved along each
//! track's racing line (the position-node graph) for several laps.

mod common;

use glam::Vec3;
use revolt_web::race::{LapEvent, LapTracker, Track};

/// Racing-line points for one lap, from the node nearest the start position.
fn lap_path(level: &revolt_web::assets::LevelAssets) -> Vec<Vec3> {
    let nodes = &level.pos_nodes.nodes;
    let start = Vec3::from(level.info.start_pos);
    let mut i = (0..nodes.len())
        .min_by(|&a, &b| Vec3::from(nodes[a].pos).distance(start).total_cmp(&Vec3::from(nodes[b].pos).distance(start)))
        .unwrap();
    // Follow the first `next` link until the loop closes.
    let first = i;
    let mut path = Vec::new();
    for _ in 0..nodes.len() * 2 {
        path.push(Vec3::from(nodes[i].pos));
        match nodes[i].next.iter().find(|&&n| n >= 0) {
            Some(&n) => i = n as usize,
            None => break,
        }
        if i == first {
            break;
        }
    }
    assert_eq!(i, first, "{}: racing line does not loop back to the start", level.id);
    path
}

#[test]
fn counts_laps_and_progress_on_every_race_track() {
    let Some(game) = common::open_game() else { return };
    for entry in pollster::block_on(game.list_levels()).unwrap() {
        let level = pollster::block_on(game.load_level(&entry.id)).unwrap();
        let Some(track) = Track::new(level.zones.clone(), level.pos_nodes.clone()) else {
            println!("{:14} not a race track", entry.id);
            continue;
        };
        let path = lap_path(&level);
        let start = Vec3::from(level.info.start_pos);
        let mut tracker = LapTracker::new(&track, start);
        let first = path[0];
        let mut laps = Vec::new();
        let mut last_progress = f32::MIN;
        let mut backwards = 0;
        let mut worst_drop = 0.0f32;
        let mut looped = path.clone();
        looped.push(first);
        for lap in 0..3 {
            for (k, w) in looped.windows(2).enumerate() {
                for s in 0..8 {
                    let p = w[0].lerp(w[1], s as f32 / 8.0);
                    let was_wrong = tracker.wrong_way;
                    if let Some(LapEvent::Lap { n, .. }) = tracker.update(&track, p, 0.1) {
                        laps.push(n);
                    }
                    if tracker.wrong_way && !was_wrong {
                        println!("  {} wrong way at lap {lap} segment {k}: zones here {:?}", entry.id, track.zones_at(p));
                    }
                    let prog = tracker.progress();
                    if last_progress != f32::MIN && prog + 50.0 < last_progress {
                        backwards += 1;
                        if last_progress - prog > 5000.0 {
                            println!("  {} drop {:.0} -> {prog:.0} at lap {lap} segment {k} zones {:?}", entry.id, last_progress, track.zones_at(p));
                        }
                        worst_drop = worst_drop.max(last_progress - prog);
                    }
                    last_progress = prog;
                    let _ = (lap, k);
                }
            }
        }
        println!("{:14} zones {:2} laps {:?} progress {:.0} drops {backwards} (worst {worst_drop:.0})", entry.id, level.zones.len(), laps, last_progress);
        assert!(laps.len() >= 2, "{}: counted laps {laps:?}", entry.id);
        // A drop of more than a lap means a car would swap places wrongly.
        assert!(worst_drop < track.lap_length() * 0.25, "{}: progress dropped by {worst_drop}", entry.id);
        assert!(!tracker.wrong_way, "{}: flagged wrong way while driving forwards", entry.id);
    }
}
