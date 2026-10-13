# Re-Volt PC v1.00 data formats

Layouts below were checked against the shipped USA PC disc (REVOLT.EXE built
1999-07-27). All values are little-endian. Game space is right-handed with
**Y down** and Z forward; the engine converts to GL space with `(x, -y, -z)`.

## Disc

| Track | Contents |
|---|---|
| 1 | MODE1/2352 data, ISO9660. Game files under `REVOLT/`, installer at root, SecuROM-wrapped exe at `SECUROM/REVOLT.EXE` |
| 2–15 | Red Book CD audio (music; each level's `.inf` `REDBOOK a b` names its tracks) |

## `.prm` / `.m` mesh

```
u16 poly_count, u16 vert_count
Poly[poly_count]     60 bytes each
  u16 flags          0x001 quad, 0x002 double-sided, 0x004 semi-transparent,
                     0x100 additive, 0x800 (.w only) has env colour
  i16 texture        page index, -1 = untextured
  u16 index[4]       4th unused for triangles
  u32 color[4]       0xAARRGGBB, multiplied with the texture (white = unlit)
  f32 uv[4][2]
Vertex[vert_count]   24 bytes each: f32 pos[3], f32 normal[3]
```

Front faces wind **clockwise** (verified: cross product opposes stored normals).
Car wheel `.prm` files append lower-detail meshes after the first.

## `.w` world

```
i32 mesh_count
Mesh[mesh_count]: f32 center[3], f32 radius, f32 bbox[6] (xmin xmax ymin ymax zmin zmax), then a .prm mesh body
i32 ball_count
Ball[ball_count]: f32 center[3], f32 radius, i32 n, i32 mesh_index[n]
i32 unknown (0)
u32 env_color[count of polys with flag 0x800]   (no count field)
```

Level texture page N is `levels/<id>/<id><'A'+N>.bmp` (256×256, 24-bit). Pure black is the colour key.

## `.fin` instances (72 bytes each, after `i32 count`)

```
char name[9]          model = levels/<id>/<name>.prm
i8   rgb_offset[3]    added to vertex colours
u32  env_rgb
u8   priority, u8 flags, u8 pad[2]
f32  lod_bias
f32  pos[3]
f32  rot[3][3]        rows right, up, look: p' = x*right + y*up + z*look + pos
```

## `.ncp` collision

Confirmed against every file on the disc and against the game's loader
(REVOLT.EXE `sub_46b240`: i16 count, then 0x70-byte records; bit 0 counted
as quad; each bbox grown by 2.0 after loading). Several prop files are 0 bytes.

```
i16 poly_count
CollPoly[poly_count]   112 bytes each
  i32 type             0x1 quad, 0x4 camera ignores, 0x8 cars/objects ignore (camera-only),
                       0x20 no skid marks; 0x2 0x400 0x800 occur but no reader found (docs/physics.md §5)
  i32 material         surface type, 0..25
  f32 plane[4]         n.xyz, d: dot(n, p) + d > 0 outside; n points out of the surface
  f32 edge[4][4]       point is inside when dot(e.xyz, p) + e.w <= 0 for every edge (3 for triangles)
  f32 bbox[6]          xmin xmax ymin ymax zmin zmax
// level files only:
f32 x0, z0, x_cells, z_cells, cell_size   (NHOOD1: -18944, -22784, 142, 193, 256)
per cell (z-major): i32 n, i32 poly_index[n]
```

Level props add their own `<model>.ncp`, transformed by the `.fin` placement.

## `.hul` car hulls

Verified on every car: 33 files parse to the exact byte; MOUSE has 644 bytes
of junk after a valid hull. The file is named by the car's `COLL` line (Candy
uses `candy.hul`; its `hull.hul` is empty). The trolley has none.

```
u16 hull_count                  1-3 convex pieces
Hull[hull_count]:
  u16 vert_count, edge_count, face_count
  f32 bbox[6]
  f32 offset[3]
  f32 vert[vert_count][3]
  u16 edge[edge_count][2]       vertex index pairs
  f32 plane[face_count][4]      normal, d
u16 sphere_count                4-34
f32 sphere[sphere_count][4]     centre xyz, radius (car space)
```

The engine collides the spheres with the world. What the game uses the convex
hulls for (probably car-to-car contact) is not confirmed yet.

## `.taz` track zones (lap counting)

Verified on every level: `i32 count`, then 64-byte zones
`{i32 id, f32 pos[3], f32 rot[3][3] (rows = box axes), f32 half_size[3]}`.
Ids run 0..N-1 in racing order; boxes overlap where the track doubles back
and parallel boxes on forks share an id. Arenas have one zone.

## `.pan` position nodes (standings)

Verified on every level: `i32 count, i32 start_node, f32 lap_length`, then
48-byte nodes `{f32 pos[3], f32 dist_to_finish, i32 prev[4], i32 next[4]}`
(-1 = unused; links may sit in any slot). `dist_to_finish` is 0 at the finish
line and decreases along the racing direction; one link on Botanical Garden
(67→68) increases by 1,560 units. Arenas have none (or 4-byte files).

## `.fan` AI nodes (racing line)

Verified on every level (all links valid): `u16 count, u16 routes`, then
76-byte nodes, then `i32 start_node, f32 lap_length`.

```
u8  priority, u8 flags[3]       meaning not known (flags 0,3,3 on ~15% of nodes)
f32 racing_line                 0 = left edge, 1 = right edge
f32 dist                        distance along the lap
f32 overtake_line
i32 kind, i32 speed[2]          1, 30, 30 on every node seen
i32 prev[2], i32 next[2]        forks have two next links
i32 ?, f32 left[3]              left track edge
i32 ?, f32 right[3]             right track edge
```

Some fork branches continue from the top of a jump (e.g. Botanical Garden
node 334 → 168 is 147 units higher).

## Audio

Tracks 2–15 are 44.1 kHz 16-bit stereo little-endian PCM. A level's
`REDBOOK a b` plays CD tracks a+2..b+2, starting at a random one and then
looping from a+2 (confirmed in REVOLT.EXE, docs/weapons.md). Sound effects are WAV files in
`REVOLT/WAVS`.

## Text formats

* `.inf`: `NAME 'x'`, `STARTPOS x y z`, `STARTROT turns`, `FARCLIP`, `FOGSTART`, `FOGCOLOR r g b`, `REDBOOK a b`. `;` starts a comment.
* `cars/<id>/parameters.txt`: whole file wrapped in `{ }`. `MODEL n "path"` (19 slots, `"NONE"` empty), `TPAGE`, `TopSpeed` (mph), `SteerRate`, `BODY { ModelNum, Offset, Mass, Inertia (3 rows over 3 lines), ... }`, `WHEEL n { ModelNum, Offset1, Offset2, IsPresent, IsPowered, IsTurnable, SteerRatio, Radius, MaxPos, StaticFriction, KineticFriction, ... }`, `SPRING n { Stiffness, Damping, Restitution }`. `SteerRatio` is set on non-turnable wheels too (e.g. `+1.0` on rear wheels) and must be ignored there; only SGT and GENCAR steer their rear wheels.
* `.m` models (trolley, aerial, frontend props) use the `.prm` layout.

## Not yet decoded

`.fob` objects (pickups and level objects), `.tri` triggers, `.lit`, `.vis`, `.cam`, `.por`.
