//! Re-Volt in the browser: reads the user's own PC game disc and renders it
//! with WebGL2. Format parsing and scene building are platform-independent so
//! they can be tested natively; everything browser-specific is in `web`.

pub mod ai;
pub mod assets;
pub mod camera;
pub mod car;
pub mod collision;
pub mod materials;
pub mod parts;
pub mod race;
pub mod disc;
pub mod formats;
pub mod frontend;
pub mod scene;
pub mod weapons;

#[cfg(target_arch = "wasm32")]
mod web;
