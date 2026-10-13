//! Parses every level and car on a real disc image. Point REVOLT_DISC at the
//! data track (Track 01 .bin or an .iso); the test is skipped if absent.

mod common;

use revolt_web::scene;

#[test]
fn loads_every_level_and_car() {
    let Some(game) = common::open_game() else { return };
    pollster::block_on(async {

        let levels = game.list_levels().await.unwrap();
        assert!(levels.len() >= 17, "found {} levels", levels.len());
        for entry in &levels {
            let level = game.load_level(&entry.id).await.unwrap_or_else(|e| panic!("{}: {e}", entry.id));
            let tris = scene::level_scene(&level).triangle_count();
            println!(
                "{:14} {:22} meshes {:5} instances {:4} textures {:2} tris {:6} coll {:5} prop-coll {}",
                entry.id, entry.name, level.world.meshes.len(), level.instances.len(), level.textures.len(), tris,
                level.collision.len(), level.instance_collision.len()
            );
            assert!(tris > 0);
            assert!(!level.textures.is_empty(), "{} has no textures", entry.id);
            assert!(!level.collision.is_empty(), "{} has no collision", entry.id);
        }

        let cars = game.list_cars().await.unwrap();
        assert!(cars.len() >= 28, "found {} cars", cars.len());
        for entry in &cars {
            let car = game.load_car(&entry.id).await.unwrap_or_else(|e| panic!("{}: {e}", entry.id));
            println!("{:10} {:18} parts {} texture {}", entry.id, entry.name, car.parts.len(), car.texture.is_some());
            assert!(!car.parts.is_empty(), "{} has no body", entry.id);
            // The trolley is a hidden car shipped without a hull.
            assert!(car.hull.is_some() || entry.id == "TROLLEY", "{} has no hull", entry.id);
        }
    });
}
