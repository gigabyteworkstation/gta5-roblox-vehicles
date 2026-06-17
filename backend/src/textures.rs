//! .ytd texture dictionary → decoded RGBA8.
//!
//! parse_ytd (from rpf-archive) gives us each texture's format + raw GPU bytes
//! (all mips concatenated, level-0 = stride*height bytes). We decode level-0 of
//! block-compressed formats (BC1/2/3/4/5/7) to straight RGBA8 — the form
//! Roblox's EditableImage:WritePixelsBuffer wants.

use crate::rsc7::Rsc7;
use anyhow::Result;
use rpf_archive::TextureFormat;

// grcTexture (gen8) field offsets, and TextureDictionary list offsets.
const ROOT: u64 = 0x5000_0000;
const DICT_TEX_LIST_PTR: usize = 0x30;
const DICT_TEX_LIST_COUNT: usize = 0x38; // u16 (NOT u32 — the crate's parse_ytd bug)
const TEX_NAME_PTR: usize = 0x28;
const TEX_WIDTH: usize = 0x50;
const TEX_HEIGHT: usize = 0x52;
const TEX_STRIDE: usize = 0x56;
const TEX_FORMAT: usize = 0x58;
const TEX_LEVELS: usize = 0x5D;
const TEX_DATA_PTR: usize = 0x70;

pub struct DecodedTexture {
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub format: TextureFormat,
    pub rgba: Vec<u8>, // width*height*4, R,G,B,A
}

/// texture2ddecoder emits u32 as 0xAARRGGBB; split into R,G,B,A bytes.
fn argb_u32_to_rgba(img: &[u32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(img.len() * 4);
    for &p in img {
        out.push(((p >> 16) & 0xFF) as u8); // R
        out.push(((p >> 8) & 0xFF) as u8); // G
        out.push((p & 0xFF) as u8); // B
        out.push(((p >> 24) & 0xFF) as u8); // A
    }
    out
}

/// Parse a TextureDictionary (.ytd) directly off the RSC7 paged reader.
/// Avoids the crate's parse_ytd, which misreads the texture count as u32.
pub fn decode_dictionary(rsc: &Rsc7) -> Result<Vec<DecodedTexture>> {
    let list_ptr = rsc.u64_at(ROOT, DICT_TEX_LIST_PTR)?;
    let count = rsc.u16_at(ROOT, DICT_TEX_LIST_COUNT)? as usize;
    if count == 0 {
        return Ok(vec![]);
    }
    let tex_ptrs = rsc.ptr_array(list_ptr, count)?;

    let mut out = Vec::with_capacity(count);
    for &tva in &tex_ptrs {
        if tva == 0 {
            continue;
        }
        let name = rsc.str_at(rsc.u64_at(tva, TEX_NAME_PTR)?);
        let width = rsc.u16_at(tva, TEX_WIDTH)? as usize;
        let height = rsc.u16_at(tva, TEX_HEIGHT)? as usize;
        let stride = rsc.u16_at(tva, TEX_STRIDE)? as usize;
        let format = TextureFormat::from_u32(rsc.u32_at(tva, TEX_FORMAT)?);
        let _levels = rsc.u8_at(tva, TEX_LEVELS)?;
        let data_ptr = rsc.u64_at(tva, TEX_DATA_PTR)?;
        if width == 0 || height == 0 || data_ptr == 0 {
            continue;
        }

        // Level 0 = stride*height bytes (pixel data lives in the graphics segment).
        let level0 = stride * height;
        let data = match rsc.at(data_ptr, level0) {
            Ok(d) => d,
            Err(_) => {
                eprintln!("  [skip] {name}: pixel data out of bounds");
                continue;
            }
        };

        let mut img = vec![0u32; width * height];
        let decoded = match format {
            TextureFormat::DXT1 => texture2ddecoder::decode_bc1(data, width, height, &mut img).is_ok(),
            TextureFormat::DXT3 => texture2ddecoder::decode_bc2(data, width, height, &mut img).is_ok(),
            TextureFormat::DXT5 => texture2ddecoder::decode_bc3(data, width, height, &mut img).is_ok(),
            TextureFormat::ATI1 => texture2ddecoder::decode_bc4(data, width, height, &mut img).is_ok(),
            TextureFormat::ATI2 => texture2ddecoder::decode_bc5(data, width, height, &mut img).is_ok(),
            TextureFormat::BC7 => texture2ddecoder::decode_bc7(data, width, height, &mut img).is_ok(),
            TextureFormat::A8R8G8B8 | TextureFormat::X8R8G8B8 => {
                for (i, px) in img.iter_mut().enumerate() {
                    let o = i * 4;
                    if o + 3 < data.len() {
                        *px = u32::from_le_bytes([data[o], data[o + 1], data[o + 2], data[o + 3]]);
                    }
                }
                true
            }
            _ => false,
        };
        if !decoded {
            eprintln!("  [skip] {name}: unsupported format {format:?}");
            continue;
        }

        out.push(DecodedTexture {
            name,
            width: width as u32,
            height: height as u32,
            format,
            rgba: argb_u32_to_rgba(&img),
        });
    }
    Ok(out)
}

pub fn write_png(tex: &DecodedTexture, path: &std::path::Path) -> Result<()> {
    let img = image::RgbaImage::from_raw(tex.width, tex.height, tex.rgba.clone())
        .ok_or_else(|| anyhow::anyhow!("rgba buffer size mismatch"))?;
    img.save(path)?;
    Ok(())
}

pub fn sanitize(name: &str) -> String {
    if name.is_empty() {
        return "unnamed".into();
    }
    name.chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '-' { c } else { '_' })
        .collect()
}
