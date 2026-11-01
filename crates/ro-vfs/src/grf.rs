use std::{
    collections::HashMap,
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::Path,
    sync::Mutex,
};

use anyhow::{bail, ensure, Context};
use encoding_rs::EUC_KR;
use flate2::read::ZlibDecoder;

const HEADER_LEN: u64 = 46;
const FLAG_FILE: u8 = 0x01;
const FLAG_MIXED_CRYPT: u8 = 0x02;
const FLAG_HEADER_CRYPT: u8 = 0x04;

struct Entry {
    name: String,
    comp_len: u32,
    aligned_len: u32,
    real_len: u32,
    flags: u8,
    offset: u64,
}

/// GRF archive: 0x200 and 0x300, and the older 0x1xx (0x102 / 0x103) with obfuscated names.
pub struct Grf {
    file: Mutex<File>,
    entries: HashMap<String, Entry>,
}

impl Grf {
    pub fn open(path: &Path) -> anyhow::Result<Self> {
        let mut f = File::open(path).with_context(|| format!("open {}", path.display()))?;
        let mut h = [0u8; HEADER_LEN as usize];
        f.read_exact(&mut h)?;
        // Newer official clients replace the magic with "Event Horizon"; the layout is unchanged.
        ensure!(
            h.starts_with(b"Master of Magic") || h.starts_with(b"Event Horizon\0"),
            "{} is not a GRF",
            path.display()
        );
        let rd = |o: usize| u32::from_le_bytes(h[o..o + 4].try_into().unwrap());
        let version = rd(42);
        if version >> 8 == 0x01 {
            f.seek(SeekFrom::Start(HEADER_LEN + rd(30) as u64))?;
            let mut list = vec![];
            f.read_to_end(&mut list)?;
            let entries = old_entries(&list, rd(38).wrapping_sub(rd(34)).wrapping_sub(7))?;
            tracing::info!(path = %path.display(), files = entries.len(), "opened GRF {version:#x}");
            return Ok(Self { file: Mutex::new(f), entries });
        }
        // 0x200: u32 table offset, seed, count + seed + 7.
        // 0x300: u64 table offset, plain count; the table gains a u32 prefix and entries a u64 offset.
        let (table_off, count, wide) = match version {
            0x200 => (rd(30) as u64, rd(38).wrapping_sub(rd(34)).wrapping_sub(7), false),
            0x300 => (u64::from_le_bytes(h[30..38].try_into()?), rd(38), true),
            v => bail!("GRF version {v:#x} unsupported (0x1xx, 0x200 and 0x300 only)"),
        };

        f.seek(SeekFrom::Start(HEADER_LEN + table_off + if wide { 4 } else { 0 }))?;
        let mut sz = [0u8; 8];
        f.read_exact(&mut sz)?;
        let comp = u32::from_le_bytes(sz[0..4].try_into()?) as usize;
        let real = u32::from_le_bytes(sz[4..8].try_into()?) as usize;
        let mut cbuf = vec![0; comp];
        f.read_exact(&mut cbuf)?;
        let mut table = Vec::with_capacity(real);
        ZlibDecoder::new(&cbuf[..]).read_to_end(&mut table)?;

        let mut entries = HashMap::with_capacity(count as usize);
        let mut i = 0;
        while i < table.len() {
            let nul = table[i..].iter().position(|&b| b == 0).context("truncated file table")?;
            let end = i + nul;
            let meta = if wide { 21 } else { 17 };
            ensure!(end + 1 + meta <= table.len(), "truncated file table");
            let (name, _, _) = EUC_KR.decode(&table[i..end]);
            let m = &table[end + 1..end + 1 + meta];
            let r = |o: usize| u32::from_le_bytes(m[o..o + 4].try_into().unwrap());
            let offset = if wide { u64::from_le_bytes(m[13..21].try_into().unwrap()) } else { r(13) as u64 };
            let e = Entry { name: name.into_owned(), comp_len: r(0), aligned_len: r(4), real_len: r(8), flags: m[12], offset };
            entries.insert(crate::normalize(&e.name), e);
            i = end + 1 + meta;
        }
        tracing::info!(path = %path.display(), files = entries.len(), "opened GRF");
        Ok(Self { file: Mutex::new(f), entries })
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Original (decoded) file names.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.entries.values().filter(|e| e.flags & FLAG_FILE != 0).map(|e| e.name.as_str())
    }

    pub fn read(&self, path: &str) -> anyhow::Result<Option<Vec<u8>>> {
        let Some(e) = self.entries.get(&crate::normalize(path)) else {
            return Ok(None);
        };
        if e.flags & FLAG_FILE == 0 {
            return Ok(None);
        }
        let mut buf = vec![0; e.aligned_len as usize];
        {
            let mut f = self.file.lock().unwrap();
            f.seek(SeekFrom::Start(HEADER_LEN + e.offset))?;
            f.read_exact(&mut buf)?;
        }
        if e.flags & FLAG_MIXED_CRYPT != 0 {
            crate::des::decode_mixed(&mut buf, e.comp_len);
        } else if e.flags & FLAG_HEADER_CRYPT != 0 {
            crate::des::decode_header(&mut buf);
        }
        buf.truncate(e.comp_len as usize);
        let mut out = Vec::with_capacity(e.real_len as usize);
        ZlibDecoder::new(&buf[..]).read_to_end(&mut out)?;
        Ok(Some(out))
    }
}

/// GRF 0x1xx's file table: not compressed, names obfuscated, sizes offset by constants.
/// Each entry is `u32 n`, 2 bytes, the name (`n - 6` bytes), 4 bytes, then 17 bytes like
/// 0x200's. Its files are encrypted by extension, not by flag. As rAthena's grfio.cpp.
fn old_entries(list: &[u8], count: u32) -> anyhow::Result<HashMap<String, Entry>> {
    let mut entries = HashMap::with_capacity(count as usize);
    let r = |o: usize| list.get(o..o + 4).map(|b| u32::from_le_bytes(b.try_into().unwrap())).context("truncated file table");
    let mut ofs = 0;
    for _ in 0..count {
        let at = ofs;
        let meta = at + r(at)? as usize + 4;
        ensure!(meta + 17 <= list.len(), "truncated file table");
        ofs = meta + 17;
        let mut flags = list[meta + 12];
        if flags & FLAG_FILE == 0 {
            continue;
        }
        // rAthena takes the name's length from the low byte alone.
        let mut name = list.get(at + 6..at + (list[at] as usize).max(6)).context("truncated file table")?.to_vec();
        crate::des::decode_name(&mut name);
        name.truncate(name.iter().position(|&b| b == 0).unwrap_or(name.len()));
        let name = EUC_KR.decode(&name).0.into_owned();
        let header_only = name.rsplit_once('.').is_some_and(|(_, ext)| ["gnd", "gat", "act", "str"].iter().any(|e| ext.eq_ignore_ascii_case(e)));
        flags |= if header_only { FLAG_HEADER_CRYPT } else { FLAG_MIXED_CRYPT };
        let real_len = r(meta + 8)?;
        let (Some(comp_len), Some(aligned_len)) = (r(meta)?.checked_sub(real_len).and_then(|n| n.checked_sub(715)), r(meta + 4)?.checked_sub(37579)) else {
            tracing::warn!(%name, "GRF entry with impossible sizes, skipped");
            continue;
        };
        let e = Entry { name, comp_len, aligned_len, real_len, flags, offset: r(meta + 13)? as u64 };
        entries.insert(crate::normalize(&e.name), e);
    }
    Ok(entries)
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    /// A one-file GRF 0x103 built the way rAthena reads one. Its single DES round undoes
    /// itself, so encrypting is decrypting; the file is too short to reach the shuffled blocks.
    #[test]
    fn reads_version_0x103() {
        let body = b"old archives still open";
        let mut z = flate2::write::ZlibEncoder::new(vec![], flate2::Compression::default());
        z.write_all(body).unwrap();
        let mut data = z.finish().unwrap();
        let comp_len = data.len() as u32;
        data.resize(data.len().next_multiple_of(8), 0);
        crate::des::decode_mixed(&mut data, comp_len);

        let mut name = b"data\\Test.txt".to_vec();
        name.resize(16, 0);
        // Decoding swaps nibbles, then decrypts; so encoding decrypts, then swaps.
        let swap = |b: &mut [u8]| b.iter_mut().for_each(|b| *b = b.rotate_left(4));
        for block in name.chunks_exact_mut(8) {
            swap(block);
            crate::des::decode_name(block);
            swap(block);
        }
        let mut table = vec![];
        table.extend((name.len() as u32 + 6).to_le_bytes());
        table.extend([0, 0]);
        table.extend(&name);
        table.extend([0; 4]);
        table.extend((comp_len + body.len() as u32 + 715).to_le_bytes());
        table.extend((data.len() as u32 + 37579).to_le_bytes());
        table.extend((body.len() as u32).to_le_bytes());
        table.push(super::FLAG_FILE);
        table.extend(0u32.to_le_bytes());

        let mut grf = b"Master of Magic\0".to_vec();
        grf.resize(30, 0);
        grf.extend((data.len() as u32).to_le_bytes()); // table offset
        grf.extend(5u32.to_le_bytes()); // seed
        grf.extend((1u32 + 5 + 7).to_le_bytes());
        grf.extend(0x103u32.to_le_bytes());
        grf.extend(&data);
        grf.extend(&table);
        let path = std::env::temp_dir().join(format!("ro-vfs-test-{}.grf", std::process::id()));
        std::fs::write(&path, &grf).unwrap();
        let opened = super::Grf::open(&path);
        std::fs::remove_file(&path).ok();
        let g = opened.unwrap();
        assert_eq!(g.names().collect::<Vec<_>>(), ["data\\Test.txt"]);
        assert_eq!(g.read("data/test.txt").unwrap().as_deref(), Some(&body[..]));
    }
}
