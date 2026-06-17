# gta-vehicle backend (Rust)

Owns the GTA5 archives, decodes vehicle meshes + textures, and serves a compact
wire format to the Roblox client. Roblox never touches RPF.

`GET /rpf/readVehicle?v=zion_hi` → mesh + textures + wheel transforms.

## Layer map (what's reused vs ours)

| Layer | Job | Where |
|---|---|---|
| 1. Archive | RPF7 open, AES TOC, list/find/extract entries | `rpf-archive` crate (`Archive::open/list_files/find_file/extract`) |
| 2. Resource | RSC7 deflate + NG decrypt, paged memory rebuild | `rpf-archive` crate |
| 3. **Model** | `.yft` → fragment → drawable → geometry, decode FVF vertex/index buffers | **ours** (`src/yft/`) — port from GTA5 source + CodeWalker |
| 4. Texture | `.ytd` → `YtdTexture` (`parse_ytd`) → BC1/3/5 → RGBA | `parse_ytd` + **our DXT→RGBA decode** (`texpresso`/`squish`) |
| 5. Wire | pack geometries + RGBA textures + wheel bone CFrames | **ours** (`src/wire.rs`) |

Vendored fork: `vendor/rpf-cli` (pkg `rage-package-format`). Its reusable parts —
`src/rpf.rs` (Archive wrapper) and `src/crypto/` (GtaKeys, AES/NG) — get exposed
as a lib and depended on by our server crate. The crate carries **no keys**:
`GtaKeys` loads from `.dat` files or extracts them from `GTA5.exe` at startup.

### Confirmed upstream API (from vendor/rpf-cli/src/rpf.rs)
```rust
Archive::open(path, Option<&GtaKeys>) -> Result<Archive>
Archive::from_bytes(data, name, Option<&GtaKeys>) -> Result<Archive>   // for nested rpf
archive.list_files() -> Vec<&FileRef>
archive.find_file(path) -> Option<&FileRef>
archive.extract(file, Option<&GtaKeys>) -> Result<Vec<u8>>            // RSC7 raw bytes
archive.entry_kind(file) -> &RpfEntryKind                              // File / ResourceFile{..}
GtaKeys::load_from_path(dir) | GtaKeys::extract_from_exe(exe, save_to)
parse_ytd(rsc7_bytes) -> Vec<YtdTexture>                               // name, name_hash, TextureFormat...
```

## Milestone 1 — list + extract one vehicle's raw files (no decoding yet)

Goal: prove layers 1–2 against the real archive before writing the `.yft` decoder.

```
load GtaKeys (from .dat dir, or extract from GTA5.exe once)
open archive:
   • direct:  vehicles.rpf                      (user already extracted this)
   • nested:  x64e.rpf → find levels/gta5/vehicles.rpf → extract bytes → from_bytes
for v = "zion_hi":
   find  zion_hi.yft                 → extract → report size + entry_kind  → dump to ./out
   find  zion_hi+hifr.ytd (HD txd)   → extract → report size              → dump to ./out
   (fall back to zion.yft / zion+hi.ytd if _hi assets absent)
```
Deliverable: a `cargo run -- readVehicle zion_hi` (CLI first, axum endpoint second)
that writes the raw RSC7 bytes and prints sizes — visually confirmable.

Asset-name resolution comes from the `_manifest.ymt` HD-binding table:
`zion` (base frag) / `zion_hi` (HD frag) / `zion+hi`,`zion+hifr` (HD txds).

## Prerequisites (blocking)

1. **Rust toolchain** — not currently installed on this machine.
2. **GTA5 keys** — path to `GTA5.exe`, *or* pre-extracted
   `gtav_aes_key.dat` / `gtav_ng_key.dat` / `gtav_ng_decrypt_tables.dat`.
3. **Archive path** — the extracted `vehicles.rpf`, or `x64e.rpf`.

## Out of scope
Physics, vehicle audio, granular sound.
