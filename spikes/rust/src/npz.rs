//! Minimal reader for `voices-v1.0.bin`: a NumPy `.npz` (a stored, uncompressed zip) with one
//! `<voice>.npy` per voice, each a little-endian float32 array of shape (510, 1, 256).

use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;

use anyhow::{Context, Result, bail};

pub fn list(path: &Path) -> Result<Vec<String>> {
    let zip = zip::ZipArchive::new(BufReader::new(File::open(path)?))?;
    let mut names: Vec<String> = zip.file_names().filter_map(|n| n.ok()?.strip_suffix(".npy").map(String::from)).collect();
    names.sort();
    Ok(names)
}

/// Returns (shape, data) for `<name>.npy`.
pub fn read(path: &Path, name: &str) -> Result<(Vec<usize>, Vec<f32>)> {
    let mut zip = zip::ZipArchive::new(BufReader::new(File::open(path)?))?;
    let mut entry = zip.by_name(&format!("{name}.npy")).with_context(|| format!("no voice '{name}' in {}", path.display()))?;
    let mut buf = Vec::with_capacity(entry.size() as usize);
    entry.read_to_end(&mut buf)?;
    parse_npy(&buf)
}

fn parse_npy(b: &[u8]) -> Result<(Vec<usize>, Vec<f32>)> {
    if b.len() < 10 || &b[..6] != b"\x93NUMPY" {
        bail!("not a .npy file");
    }
    let (hlen, hstart) = match b[6] {
        1 => (u16::from_le_bytes([b[8], b[9]]) as usize, 10),
        _ => (u32::from_le_bytes([b[8], b[9], b[10], b[11]]) as usize, 12),
    };
    let header = std::str::from_utf8(&b[hstart..hstart + hlen])?;
    if !header.contains("'descr': '<f4'") || header.contains("'fortran_order': True") {
        bail!("unsupported npy header: {header}");
    }
    let shape_src = header.split("'shape': (").nth(1).and_then(|s| s.split(')').next()).context("npy shape")?;
    let shape: Vec<usize> = shape_src.split(',').filter_map(|s| s.trim().parse().ok()).collect();
    let data: Vec<f32> = b[hstart + hlen..].chunks_exact(4).map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect();
    if data.len() != shape.iter().product::<usize>() {
        bail!("npy size mismatch");
    }
    Ok((shape, data))
}
