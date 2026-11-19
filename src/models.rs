//! The model catalogue, first-run download and checksum verification.
//!
//! Files live in `$SPEAK_HOME/models/` under the names kokoro-onnx's release uses, so an existing
//! speak.py install is picked up as-is. Downloads go to `<file>.part` (resumed with an HTTP range
//! request if one is left over), are checked against a pinned SHA-256 and only then renamed into
//! place. A file that verified once is remembered by size and modification time in the cache
//! directory, so start-up doesn't hash 300 MB every time.

use std::fs::{self, File, OpenOptions};
use std::io::{IsTerminal, Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use sha2::{Digest, Sha256};

use crate::backend::{Files, Missing, Unusable};

pub struct Asset {
    pub file: &'static str,
    pub url: &'static str,
    pub sha256: &'static str,
    pub size: u64,
}

pub struct Model {
    pub name: &'static str,
    pub backend: &'static str,
    pub summary: &'static str,
    pub onnx: Asset,
    pub voices: Asset,
}

impl Model {
    pub fn assets(&self) -> [&Asset; 2] {
        [&self.onnx, &self.voices]
    }

    pub fn size(&self) -> u64 {
        self.onnx.size + self.voices.size
    }
}

// kokoro-onnx's `model-files-v1.0` release (Apache-2.0 weights from hexgrad/Kokoro-82M). The
// release publishes no digests: these SHA-256s were computed from downloads on 2026-11-19 and the
// sizes match the release API.
const VOICES: Asset = Asset {
    file: "voices-v1.0.bin",
    url: "https://github.com/thewh1teagle/kokoro-onnx/releases/download/model-files-v1.0/voices-v1.0.bin",
    sha256: "bca610b8308e8d99f32e6fe4197e7ec01679264efed0cac9140fe9c29f1fbf7d",
    size: 28_214_398,
};

/// In order of preference: the first one installed is the default.
pub const CATALOG: &[Model] = &[
    Model {
        name: "kokoro",
        backend: "kokoro",
        summary: "Kokoro-82M v1.0, fp32",
        onnx: Asset {
            file: "kokoro-v1.0.onnx",
            url: "https://github.com/thewh1teagle/kokoro-onnx/releases/download/model-files-v1.0/kokoro-v1.0.onnx",
            sha256: "7d5df8ecf7d4b1878015a32686053fd0eebe2bc377234608764cc0ef3636a6c5",
            size: 325_532_387,
        },
        voices: VOICES,
    },
    Model {
        name: "kokoro-int8",
        backend: "kokoro",
        summary: "Kokoro-82M v1.0, 8-bit quantised",
        onnx: Asset {
            file: "kokoro-v1.0.int8.onnx",
            url: "https://github.com/thewh1teagle/kokoro-onnx/releases/download/model-files-v1.0/kokoro-v1.0.int8.onnx",
            sha256: "6e742170d309016e5891a994e1ce1559c702a2ccd0075e67ef7157974f6406cb",
            size: 92_361_271,
        },
        voices: VOICES,
    },
];

pub fn find(name: &str) -> Option<&'static Model> {
    CATALOG.iter().find(|m| m.name == name)
}

pub fn names() -> String {
    CATALOG.iter().map(|m| m.name).collect::<Vec<_>>().join(", ")
}

pub fn dir(home: &Path) -> PathBuf {
    home.join("models")
}

/// All files present with the expected size (cheap; the checksum is checked on use).
pub fn is_installed(home: &Path, m: &Model) -> bool {
    m.assets().iter().all(|a| fs::metadata(dir(home).join(a.file)).is_ok_and(|md| md.len() == a.size))
}

/// The best installed model, or the catalogue's first (to be downloaded) if none is.
pub fn best(home: &Path) -> &'static Model {
    CATALOG.iter().find(|m| is_installed(home, m)).unwrap_or(&CATALOG[0])
}

pub fn files(home: &Path, m: &Model) -> Files {
    Files { model: dir(home).join(m.onnx.file), voices: dir(home).join(m.voices.file) }
}

/// Make sure `m` is installed and intact, downloading missing files when `download` is set.
/// Existing files are never modified here; one that fails its checksum makes the model unusable.
pub fn ensure(home: &Path, m: &Model, download: bool) -> Result<Files> {
    let missing: Vec<&Asset> = m.assets().into_iter().filter(|a| !dir(home).join(a.file).exists()).collect();
    if !missing.is_empty() {
        if !download {
            return Err(Missing(format!("model '{}' is not installed (run: speak models download {})", m.name, m.name)).into());
        }
        let total: u64 = missing.iter().map(|a| a.size).sum();
        eprintln!("speak: downloading {} ({}) into {}", m.name, megabytes(total), dir(home).display());
        for a in missing {
            fetch(a, &dir(home).join(a.file)).map_err(|e| Missing(format!("model '{}' could not be downloaded: {e:#}", m.name)))?;
        }
    }
    for a in m.assets() {
        let path = dir(home).join(a.file);
        if !verify(&path, a)? {
            return Err(Unusable(format!("{} doesn't match its checksum (run: speak models download {})", path.display(), m.name)).into());
        }
    }
    Ok(files(home, m))
}

/// `speak models download`: fetch what is missing or corrupt. Returns what was downloaded.
pub fn install(home: &Path, m: &Model) -> Result<Vec<PathBuf>> {
    let mut fetched = Vec::new();
    for a in m.assets() {
        let path = dir(home).join(a.file);
        if path.exists() && verify(&path, a)? {
            continue;
        }
        eprintln!("speak: downloading {} ({})", a.file, megabytes(a.size));
        fetch(a, &path)?;
        fetched.push(path);
    }
    Ok(fetched)
}

/// `speak models rm`: delete the model's files, keeping files another installed model shares.
pub fn remove(home: &Path, m: &Model) -> Result<Vec<PathBuf>> {
    let mut removed = Vec::new();
    for a in m.assets() {
        let shared = CATALOG.iter().any(|o| o.name != m.name && is_installed(home, o) && o.assets().iter().any(|b| b.file == a.file));
        let path = dir(home).join(a.file);
        if !shared && path.exists() {
            fs::remove_file(&path).with_context(|| format!("removing {}", path.display()))?;
            removed.push(path);
        }
    }
    Ok(removed)
}

pub fn megabytes(bytes: u64) -> String {
    format!("{} MB", (bytes + 500_000) / 1_000_000)
}

pub fn sha256_file(path: &Path) -> Result<String> {
    let mut f = File::open(path).with_context(|| format!("opening {}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

/// Does `path` have the asset's checksum? Uses (and records) a stamp so a file is hashed once.
pub fn verify(path: &Path, asset: &Asset) -> Result<bool> {
    let stamps = crate::paths::cache_dir().join("verified");
    let Some(key) = stamp_key(path) else { return Ok(false) };
    let line = format!("{} {key}", asset.sha256);
    if fs::read_to_string(&stamps).unwrap_or_default().lines().any(|l| l == line) {
        return Ok(true);
    }
    if fs::metadata(path)?.len() != asset.size || sha256_file(path)? != asset.sha256 {
        return Ok(false);
    }
    let _ = record_stamp(&stamps, &key, &line); // a cache: failing to write it only costs a re-hash
    Ok(true)
}

/// `size mtime path` of a file, with the path canonicalised so symlinks share one stamp.
fn stamp_key(path: &Path) -> Option<String> {
    let md = fs::metadata(path).ok()?;
    let mtime = md.modified().ok()?.duration_since(std::time::UNIX_EPOCH).ok()?.as_nanos();
    let canonical = fs::canonicalize(path).ok()?;
    Some(format!("{} {mtime} {}", md.len(), canonical.display()))
}

fn record_stamp(stamps: &Path, key: &str, line: &str) -> std::io::Result<()> {
    fs::create_dir_all(stamps.parent().unwrap_or(Path::new(".")))?;
    let path_part = key.splitn(3, ' ').nth(2).unwrap_or_default();
    let old = fs::read_to_string(stamps).unwrap_or_default();
    let mut lines: Vec<&str> = old.lines().filter(|l| l.splitn(4, ' ').nth(3) != Some(path_part)).collect();
    lines.push(line);
    let tmp = stamps.with_extension(format!("tmp{}", std::process::id()));
    fs::write(&tmp, lines.join("\n") + "\n")?;
    fs::rename(tmp, stamps)
}

/// Download `asset` to `dest` via `dest.part`: resume a leftover part, verify, then rename.
pub fn fetch(asset: &Asset, dest: &Path) -> Result<()> {
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)?;
    }
    let part = dest.with_file_name(format!("{}.part", asset.file));
    let mut have = fs::metadata(&part).map_or(0, |m| m.len());
    if have > asset.size {
        fs::remove_file(&part)?;
        have = 0;
    }
    if have < asset.size {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_connect(Some(Duration::from_secs(20)))
            .timeout_recv_response(Some(Duration::from_secs(60)))
            .user_agent(concat!("speak/", env!("CARGO_PKG_VERSION")))
            .build()
            .into();
        let mut req = agent.get(asset.url);
        if have > 0 {
            req = req.header("Range", format!("bytes={have}-"));
        }
        let resp = req.call().with_context(|| format!("GET {}", asset.url))?;
        if std::env::var_os("SPEAK_DEBUG").is_some() {
            eprintln!("speak: GET {} from byte {have}: HTTP {}", asset.file, resp.status().as_u16());
        }
        let mut file = match resp.status().as_u16() {
            206 => OpenOptions::new().append(true).open(&part)?,
            200 => {
                have = 0;
                File::create(&part)?
            }
            s => bail!("GET {}: HTTP {s}", asset.url),
        };
        let mut body = resp.into_body().into_reader();
        let mut buf = vec![0u8; 1 << 16];
        let progress = std::io::stderr().is_terminal();
        let mut shown = 0;
        loop {
            let n = body.read(&mut buf).with_context(|| format!("downloading {}", asset.file))?;
            if n == 0 {
                break;
            }
            file.write_all(&buf[..n])?;
            have += n as u64;
            if progress && (have - shown > asset.size / 200 || have == asset.size) {
                shown = have;
                eprint!("\r\x1b[2Kspeak: {} {} / {}", asset.file, megabytes(have), megabytes(asset.size));
            }
        }
        file.sync_all()?;
        if progress {
            eprintln!();
        }
    }
    let sha = sha256_file(&part)?;
    if sha != asset.sha256 {
        let _ = fs::remove_file(&part);
        bail!("{} failed its checksum (got {sha}, expected {})", asset.file, asset.sha256);
    }
    fs::rename(&part, dest)?;
    let _ = verify(dest, asset); // records the stamp
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalogue_is_well_formed() {
        assert_eq!(find("kokoro").unwrap().onnx.file, "kokoro-v1.0.onnx");
        assert!(find("nope").is_none());
        for m in CATALOG {
            for a in m.assets() {
                assert_eq!(a.sha256.len(), 64);
                assert!(a.url.starts_with("https://") && a.url.ends_with(a.file));
            }
        }
        assert_eq!(megabytes(325_532_387 + 28_214_398), "354 MB");
    }

    #[test]
    fn hashes_and_best_model() {
        let home = std::env::temp_dir().join(format!("speak-models-{}", std::process::id()));
        fs::create_dir_all(dir(&home)).unwrap();
        let f = home.join("abc");
        fs::write(&f, b"abc").unwrap();
        assert_eq!(sha256_file(&f).unwrap(), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        // Nothing installed: the first entry is the default. A wrong-size file doesn't count.
        assert_eq!(best(&home).name, "kokoro");
        fs::write(dir(&home).join("voices-v1.0.bin"), b"short").unwrap();
        assert!(!is_installed(&home, &CATALOG[1]));
        assert!(remove(&home, &CATALOG[0]).unwrap().len() == 1);
        fs::remove_dir_all(&home).unwrap();
    }
}
