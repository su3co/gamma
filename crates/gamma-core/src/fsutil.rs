//! Crash-safe file writes: write a sibling temporary file, flush it to disk,
//! then rename it over the target. A crash leaves either the old or the new
//! file, never a torn one.

use anyhow::{Context, Result};
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::Path;

/// Write `data` to `path` atomically (temporary file, fsync, rename, fsync of the folder).
pub fn atomic_write(path: &Path, data: &[u8]) -> Result<()> {
    let dir = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(dir).with_context(|| format!("creating folder {}", dir.display()))?;
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("file");
    let tmp = dir.join(format!(".{name}.tmp-{}", std::process::id()));
    {
        let mut f = File::create(&tmp).with_context(|| format!("creating {}", tmp.display()))?;
        f.write_all(data)?;
        f.sync_all()?;
    }
    fs::rename(&tmp, path).with_context(|| format!("replacing {}", path.display()))?;
    #[cfg(unix)]
    if let Ok(d) = File::open(dir) {
        let _ = d.sync_all();
    }
    Ok(())
}

/// Append one line to a log file and flush it to disk.
pub fn append_line(path: &Path, line: &str) -> Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let mut f = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .with_context(|| format!("opening {}", path.display()))?;
    let mut s = String::with_capacity(line.len() + 1);
    s.push_str(line);
    s.push('\n');
    f.write_all(s.as_bytes())?;
    f.sync_data()?;
    Ok(())
}

/// Read a newline-delimited log, dropping a torn last line (a crash during an
/// append) and truncating the file to the last complete line.
pub fn read_lines_repair(path: &Path) -> Result<Vec<String>> {
    let data = match fs::read(path) {
        Ok(d) => d,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
        Err(e) => return Err(e).with_context(|| format!("reading {}", path.display())),
    };
    let complete = match data.iter().rposition(|&b| b == b'\n') {
        Some(i) => i + 1,
        None => 0,
    };
    if complete < data.len() {
        let f = OpenOptions::new().write(true).open(path)?;
        f.set_len(complete as u64)?;
        f.sync_all()?;
    }
    let text = String::from_utf8_lossy(&data[..complete]);
    Ok(text
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.to_string())
        .collect())
}

/// SHA-256 of a file's bytes, hex encoded.
pub fn file_sha256(path: &Path) -> Result<String> {
    use sha2::{Digest, Sha256};
    let data = fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    Ok(hex::encode(Sha256::digest(&data)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atomic_and_repair() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("a/b.txt");
        atomic_write(&p, b"one").unwrap();
        atomic_write(&p, b"two").unwrap();
        assert_eq!(fs::read(&p).unwrap(), b"two");
        let log = d.path().join("log.jsonl");
        append_line(&log, "{\"a\":1}").unwrap();
        append_line(&log, "{\"a\":2}").unwrap();
        // torn write
        let mut f = OpenOptions::new().append(true).open(&log).unwrap();
        f.write_all(b"{\"a\":").unwrap();
        drop(f);
        let lines = read_lines_repair(&log).unwrap();
        assert_eq!(lines.len(), 2);
        assert!(fs::read_to_string(&log).unwrap().ends_with("2}\n"));
    }
}
