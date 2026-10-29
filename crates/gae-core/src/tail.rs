//! Incremental line reader: each poll returns only the complete lines appended since the last.
//! Survives the file not existing yet, being truncated or rotated, and half-written last lines.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

/// The most bytes one poll reads, so a huge backlog drains over several frames, not one.
pub const MAX_CHUNK: u64 = 4 * 1024 * 1024;

#[derive(Debug)]
pub struct FileTail {
    path: PathBuf,
    offset: u64,
    partial: Vec<u8>,
    started: bool,
    from_start: bool,
}

impl FileTail {
    /// `from_start: false` skips what is already in the file and follows new lines only.
    pub fn new(path: impl Into<PathBuf>, from_start: bool) -> FileTail {
        FileTail {
            path: path.into(),
            offset: 0,
            partial: Vec::new(),
            started: false,
            from_start,
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// New complete lines (without their line endings). A missing file is not an error: no lines.
    pub fn read_lines(&mut self) -> std::io::Result<Vec<String>> {
        let mut f = match File::open(&self.path) {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e),
        };
        let len = f.metadata()?.len();
        if !self.started {
            self.started = true;
            if !self.from_start {
                self.offset = len;
            }
        }
        if len < self.offset {
            // Truncated or rotated: start over from the top of the new file.
            self.offset = 0;
            self.partial.clear();
        }
        if len == self.offset {
            return Ok(Vec::new());
        }
        f.seek(SeekFrom::Start(self.offset))?;
        let want = (len - self.offset).min(MAX_CHUNK);
        let mut buf = Vec::with_capacity(want as usize);
        f.take(want).read_to_end(&mut buf)?;
        self.offset += buf.len() as u64;
        self.partial.extend_from_slice(&buf);
        let mut lines = Vec::new();
        let mut start = 0;
        for (i, b) in self.partial.iter().enumerate() {
            if *b == b'\n' {
                let mut line = &self.partial[start..i];
                if line.last() == Some(&b'\r') {
                    line = &line[..line.len() - 1];
                }
                if !line.is_empty() {
                    lines.push(String::from_utf8_lossy(line).into_owned());
                }
                start = i + 1;
            }
        }
        self.partial.drain(..start);
        // A single "line" larger than a chunk with no newline yet: keep it bounded.
        if self.partial.len() as u64 > MAX_CHUNK {
            self.partial.clear();
        }
        Ok(lines)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn temp(name: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("gae-tail-{}-{name}", std::process::id()));
        let _ = std::fs::remove_file(&p);
        p
    }

    #[test]
    fn reads_appended_complete_lines_only() {
        let p = temp("append");
        let mut t = FileTail::new(&p, true);
        assert!(t.read_lines().unwrap().is_empty(), "missing file is fine");
        let mut f = File::create(&p).unwrap();
        write!(f, "one\r\ntwo\nthr").unwrap();
        f.flush().unwrap();
        assert_eq!(t.read_lines().unwrap(), ["one", "two"]);
        writeln!(f, "ee").unwrap();
        f.flush().unwrap();
        assert_eq!(t.read_lines().unwrap(), ["three"]);
        assert!(t.read_lines().unwrap().is_empty());
        drop(f);
        std::fs::write(&p, "new\n").unwrap();
        assert_eq!(t.read_lines().unwrap(), ["new"], "truncation restarts");
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn follow_mode_skips_existing_content() {
        let p = temp("follow");
        std::fs::write(&p, "old\n").unwrap();
        let mut t = FileTail::new(&p, false);
        assert!(t.read_lines().unwrap().is_empty());
        let mut f = std::fs::OpenOptions::new().append(true).open(&p).unwrap();
        writeln!(f, "fresh").unwrap();
        f.flush().unwrap();
        assert_eq!(t.read_lines().unwrap(), ["fresh"]);
        let _ = std::fs::remove_file(&p);
    }
}
