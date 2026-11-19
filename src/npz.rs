//! Reader for `voices-v1.0.bin`: a NumPy `.npz` (an uncompressed zip) holding one `<voice>.npy`
//! per voice, each a little-endian float32 array of shape (510, 1, 256).

use std::fs::File;
use std::io::{BufReader, Read, Seek};
use std::path::Path;

use anyhow::{Context, Result, bail};

pub fn list(path: &Path) -> Result<Vec<String>> {
    names(File::open(path).with_context(|| format!("opening {}", path.display()))?)
}

/// Returns (shape, data) of `<name>.npy`.
pub fn read(path: &Path, name: &str) -> Result<(Vec<usize>, Vec<f32>)> {
    read_from(File::open(path).with_context(|| format!("opening {}", path.display()))?, name)
}

fn names(r: impl Read + Seek) -> Result<Vec<String>> {
    let zip = zip::ZipArchive::new(BufReader::new(r))?;
    let mut names: Vec<String> = zip.file_names().filter_map(|n| n.ok()?.strip_suffix(".npy").map(String::from)).collect();
    names.sort();
    Ok(names)
}

fn read_from(r: impl Read + Seek, name: &str) -> Result<(Vec<usize>, Vec<f32>)> {
    let mut zip = zip::ZipArchive::new(BufReader::new(r))?;
    let mut entry = zip.by_name(&format!("{name}.npy")).with_context(|| format!("no voice '{name}'"))?;
    let mut buf = Vec::with_capacity(entry.size() as usize);
    entry.read_to_end(&mut buf)?;
    parse_npy(&buf)
}

pub fn parse_npy(b: &[u8]) -> Result<(Vec<usize>, Vec<f32>)> {
    if b.len() < 10 || &b[..6] != b"\x93NUMPY" {
        bail!("not a .npy file");
    }
    let (header_len, start) = match b[6] {
        1 => (u16::from_le_bytes([b[8], b[9]]) as usize, 10),
        _ if b.len() >= 12 => (u32::from_le_bytes([b[8], b[9], b[10], b[11]]) as usize, 12),
        _ => bail!("truncated .npy header"),
    };
    let header = std::str::from_utf8(b.get(start..start + header_len).context("truncated .npy header")?)?;
    if !header.contains("'descr': '<f4'") || header.contains("'fortran_order': True") {
        bail!("unsupported .npy header: {header}");
    }
    let shape_src = header.split("'shape': (").nth(1).and_then(|s| s.split(')').next()).context("no shape in .npy header")?;
    let shape: Vec<usize> = shape_src.split(',').filter_map(|s| s.trim().parse().ok()).collect();
    let data: Vec<f32> = b[start + header_len..].chunks_exact(4).map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect();
    if data.len() != shape.iter().product::<usize>() {
        bail!(".npy size doesn't match its shape {shape:?}");
    }
    Ok((shape, data))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Cursor, Write};

    fn npy(shape: &str, data: &[f32]) -> Vec<u8> {
        let mut header = format!("{{'descr': '<f4', 'fortran_order': False, 'shape': {shape}, }}");
        while (10 + header.len() + 1) % 64 != 0 {
            header.push(' ');
        }
        header.push('\n');
        let mut b = b"\x93NUMPY\x01\x00".to_vec();
        b.extend((header.len() as u16).to_le_bytes());
        b.extend(header.as_bytes());
        b.extend(data.iter().flat_map(|f| f.to_le_bytes()));
        b
    }

    #[test]
    fn parses_npy() {
        let (shape, data) = parse_npy(&npy("(2, 1, 3)", &[1.0, 2.0, 3.0, 4.0, 5.0, 6.5])).unwrap();
        assert_eq!(shape, vec![2, 1, 3]);
        assert_eq!(data, vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.5]);
        assert!(parse_npy(&npy("(2, 2)", &[1.0])).is_err());
        assert!(parse_npy(b"PK\x03\x04 not npy").is_err());
    }

    #[test]
    fn reads_voices_from_an_npz() {
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        for (name, v) in [("bf_test.npy", 0.5f32), ("af_test.npy", 0.25)] {
            zip.start_file(name, opts).unwrap();
            zip.write_all(&npy("(2, 1, 2)", &[v; 4])).unwrap();
        }
        let bytes = zip.finish().unwrap().into_inner();
        assert_eq!(names(Cursor::new(&bytes)).unwrap(), vec!["af_test", "bf_test"]);
        let (shape, data) = read_from(Cursor::new(&bytes), "bf_test").unwrap();
        assert_eq!((shape, data), (vec![2, 1, 2], vec![0.5; 4]));
        assert!(read_from(Cursor::new(&bytes), "nope").is_err());
    }
}
