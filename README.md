# gta-vehicles

Port GTA5 vehicle **meshes** (and, soon, textures) into Roblox at runtime using
`EditableMesh`/`EditableImage`, driven by the real RAGE file formats.

A native **Rust backend** owns the GTA5 archive, decrypts + decodes a vehicle on
request, and serves a compact binary; a thin **Roblox/LuaU client** fetches it
over HTTP and builds `MeshPart`s. Roblox never touches RPF, keys, or encryption.

> This repo contains **only our own format-reading code**. It ships **no**
> Rockstar keys, no `.rpf`/`.exe`, and no decoded assets — those stay local and
> are git-ignored. You must supply your own legally-obtained game files + keys.

## Layout

```
backend/      Rust: RPF7/RSC7 → .yft mesh + .ytd textures → GVEH wire format → HTTP
roblox/       Rojo project: LuaU client that builds EditableMesh MeshParts
```

## Backend (the PC that has the game files)

Needs Rust (`rustup`) and a folder with the keys + archive:
`gtav_aes_key.dat`, `gtav_ng_key.dat`, `gtav_ng_decrypt_tables.dat`, `x64e.rpf`
(point `--keys` / `--archive` at it; defaults are in `archive.rs`).

```
cd backend
cargo run -- list wheel              # explore vehicles.rpf
cargo run -- mesh zion_hi            # decode → out/zion_hi.obj
cargo run -- textures zion.ytd       # decode → out/zion/*.png
cargo run -- serve --addr 0.0.0.0:5000   # serve to Roblox
```

`GET /rpf/readVehicle?v=zion_hi` → GVEH binary (mesh). Add `&tex=1` for textures.

## Roblox client (the PC that has Studio)

Uses [Rojo](https://rojo.space). See SETUP below.

1. Edit `roblox/src/client/init.client.luau` → set `SERVER_URL` to the backend
   machine (`http://<lan-ip>:5000`).
2. In Studio: Game Settings → Security → **Allow HTTP Requests**.
3. `rojo serve` from `roblox/`, connect via the Rojo Studio plugin, Play.

A `zion_hi` Model appears at (0, 5, 0).

## Status

- ✅ RPF7/RSC7 decrypt + inflate + paging
- ✅ `.yft` mesh decode (body + wheel physics child, correct scale)
- ✅ `.ytd` texture decode (BC1–7 → RGBA)
- ✅ HTTP server + LuaU `EditableMesh` client
- ⬜ shader→texture binding (apply correct texture per geometry)
- ⬜ wheel instancing at all 4 corners; LODs; more vehicles
- ⬜ physics (later, separate phase)

Not in scope: vehicle audio / granular sound.
