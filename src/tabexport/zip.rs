//! A small streaming ZIP writer (deflate) for the `.xlsx` export: every file is compressed
//! straight into the output; its local header is completed afterwards (the output must be
//! seekable), so even a big worksheet is never held in memory.

use flate2::Compression;
use flate2::write::DeflateEncoder;
use std::io::{self, Seek, SeekFrom, Write};

/// Compressed data is passed to the output in pieces of this size.
const CHUNK: usize = 1 << 20;

struct Entry {
    name: String,
    offset: u64,
    crc: u32,
    compressed: u64,
    size: u64,
}

struct Current {
    entry: Entry,
    hasher: crc32fast::Hasher,
    enc: DeflateEncoder<Vec<u8>>,
}

pub struct ZipWriter<W: Write + Seek> {
    out: W,
    done: Vec<Entry>,
    current: Option<Current>,
}

fn too_big() -> io::Error {
    io::Error::other("the file is too big for a ZIP without ZIP64 (4 GB)")
}

impl<W: Write + Seek> ZipWriter<W> {
    pub fn new(out: W) -> Self {
        Self {
            out,
            done: Vec::new(),
            current: None,
        }
    }

    /// Starts the next file; the data is written with `write`.
    pub fn start_file(&mut self, name: &str) -> io::Result<()> {
        self.finish_file()?;
        let offset = self.out.stream_position()?;
        // Local header; CRC and sizes are filled in by `finish_file`.
        let mut h = Vec::with_capacity(30 + name.len());
        h.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
        h.extend_from_slice(&20u16.to_le_bytes()); // version needed
        h.extend_from_slice(&0x0800u16.to_le_bytes()); // UTF-8 names
        h.extend_from_slice(&8u16.to_le_bytes()); // deflate
        h.extend_from_slice(&0u16.to_le_bytes()); // time
        h.extend_from_slice(&0x21u16.to_le_bytes()); // date 1980-01-01
        h.extend_from_slice(&[0; 12]); // crc, compressed, size
        h.extend_from_slice(&(name.len() as u16).to_le_bytes());
        h.extend_from_slice(&0u16.to_le_bytes());
        h.extend_from_slice(name.as_bytes());
        self.out.write_all(&h)?;
        self.current = Some(Current {
            entry: Entry {
                name: name.to_owned(),
                offset,
                crc: 0,
                compressed: 0,
                size: 0,
            },
            hasher: crc32fast::Hasher::new(),
            enc: DeflateEncoder::new(Vec::with_capacity(CHUNK * 2), Compression::fast()),
        });
        Ok(())
    }

    /// A whole small file at once.
    pub fn file(&mut self, name: &str, data: &[u8]) -> io::Result<()> {
        self.start_file(name)?;
        self.write_all(data)
    }

    fn finish_file(&mut self) -> io::Result<()> {
        let Some(mut c) = self.current.take() else {
            return Ok(());
        };
        let rest = c.enc.finish()?;
        self.out.write_all(&rest)?;
        c.entry.compressed += rest.len() as u64;
        c.entry.crc = c.hasher.finalize();
        let e = c.entry;
        if e.size > u32::MAX as u64 || e.compressed > u32::MAX as u64 {
            return Err(too_big());
        }
        let end = self.out.stream_position()?;
        self.out.seek(SeekFrom::Start(e.offset + 14))?;
        let mut v = Vec::with_capacity(12);
        v.extend_from_slice(&e.crc.to_le_bytes());
        v.extend_from_slice(&(e.compressed as u32).to_le_bytes());
        v.extend_from_slice(&(e.size as u32).to_le_bytes());
        self.out.write_all(&v)?;
        self.out.seek(SeekFrom::Start(end))?;
        self.done.push(e);
        Ok(())
    }

    /// Writes the central directory; returns the output.
    pub fn finish(mut self) -> io::Result<W> {
        self.finish_file()?;
        let start = self.out.stream_position()?;
        let mut cd = Vec::new();
        for e in &self.done {
            if e.offset > u32::MAX as u64 {
                return Err(too_big());
            }
            cd.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
            cd.extend_from_slice(&20u16.to_le_bytes()); // made by
            cd.extend_from_slice(&20u16.to_le_bytes()); // needed
            cd.extend_from_slice(&0x0800u16.to_le_bytes());
            cd.extend_from_slice(&8u16.to_le_bytes());
            cd.extend_from_slice(&0u16.to_le_bytes());
            cd.extend_from_slice(&0x21u16.to_le_bytes());
            cd.extend_from_slice(&e.crc.to_le_bytes());
            cd.extend_from_slice(&(e.compressed as u32).to_le_bytes());
            cd.extend_from_slice(&(e.size as u32).to_le_bytes());
            cd.extend_from_slice(&(e.name.len() as u16).to_le_bytes());
            cd.extend_from_slice(&[0; 8]); // extra, comment, disk, internal attributes
            cd.extend_from_slice(&0u32.to_le_bytes()); // external attributes
            cd.extend_from_slice(&(e.offset as u32).to_le_bytes());
            cd.extend_from_slice(e.name.as_bytes());
        }
        let n = self.done.len() as u16;
        let mut end = Vec::with_capacity(22);
        end.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
        end.extend_from_slice(&[0; 4]); // disk numbers
        end.extend_from_slice(&n.to_le_bytes());
        end.extend_from_slice(&n.to_le_bytes());
        end.extend_from_slice(&(cd.len() as u32).to_le_bytes());
        end.extend_from_slice(&(start as u32).to_le_bytes());
        end.extend_from_slice(&0u16.to_le_bytes());
        self.out.write_all(&cd)?;
        self.out.write_all(&end)?;
        self.out.flush()?;
        Ok(self.out)
    }
}

impl<W: Write + Seek> Write for ZipWriter<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let c = self
            .current
            .as_mut()
            .ok_or_else(|| io::Error::other("no ZIP entry started"))?;
        c.hasher.update(buf);
        c.entry.size += buf.len() as u64;
        c.enc.write_all(buf)?;
        if c.enc.get_ref().len() >= CHUNK {
            let data = std::mem::take(c.enc.get_mut());
            c.entry.compressed += data.len() as u64;
            self.out.write_all(&data)?;
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn written_zip_reads_back() {
        let mut z = ZipWriter::new(io::Cursor::new(Vec::new()));
        z.file("a.txt", b"hello").unwrap();
        z.start_file("dir/big.xml").unwrap();
        // More than one chunk of compressed data (random-ish text compresses badly).
        let mut big = Vec::new();
        let mut x: u32 = 1;
        for _ in 0..3_000_000 {
            x = x.wrapping_mul(1_103_515_245).wrapping_add(12345);
            big.push(b'a' + (x >> 24) as u8 % 26);
        }
        z.write_all(&big).unwrap();
        z.file("empty", b"").unwrap();
        let data = z.finish().unwrap().into_inner();
        let files = crate::update::unzip(&data).unwrap();
        let names: Vec<&str> = files.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, ["a.txt", "dir/big.xml", "empty"]);
        assert_eq!(files[0].1, b"hello");
        assert_eq!(files[1].1, big);
        assert!(files[2].1.is_empty());
    }
}
