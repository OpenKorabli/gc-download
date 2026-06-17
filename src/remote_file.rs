use std::collections::HashMap;
use std::io::{self, Read, Seek, SeekFrom};

const BLOCK_SIZE: u64 = 256 * 1024;

pub struct RemoteFile {
    url: String,
    file_size: u64,
    agent: ureq::Agent,
    cache: HashMap<u64, Vec<u8>>,
    pos: u64,
    pub requests: u64,
}

impl RemoteFile {
    pub fn new(url: &str) -> anyhow::Result<Self> {
        let agent = ureq::Agent::new_with_defaults();
        let resp = agent.head(url).call()?;
        let file_size = resp
            .headers()
            .get("Content-Length")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<u64>().ok())
            .ok_or_else(|| anyhow::anyhow!("Missing Content-Length header"))?;
        let accepts = resp
            .headers()
            .get("Accept-Ranges")
            .and_then(|v| v.to_str().ok())
            .map(|v| v == "bytes")
            .unwrap_or(false);
        if !accepts {
            anyhow::bail!("Server does not support range requests");
        }
        Ok(Self { url: url.to_string(), file_size, agent, cache: HashMap::new(), pos: 0, requests: 0 })
    }

    pub fn file_size(&self) -> u64 { self.file_size }

    pub fn fetch_range(&mut self, offset: u64, size: u64) -> anyhow::Result<Vec<u8>> {
        let range = format!("bytes={}-{}", offset, offset + size.saturating_sub(1));
        let resp = self.agent.get(&self.url).header("Range", &range).call()?;
        self.requests += 1;
        let data = resp.into_body().read_to_vec()?;
        Ok(data)
    }

    fn fetch_block(&mut self, block_idx: u64) -> anyhow::Result<()> {
        if self.cache.contains_key(&block_idx) {
            return Ok(());
        }
        let start = block_idx * BLOCK_SIZE;
        let end = (start + BLOCK_SIZE - 1).min(self.file_size - 1);
        let range = format!("bytes={}-{}", start, end);
        let resp = self.agent.get(&self.url).header("Range", &range).call()?;
        self.requests += 1;
        let data = resp.into_body().read_to_vec()?;
        self.cache.insert(block_idx, data);
        Ok(())
    }
}

impl Read for RemoteFile {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.pos >= self.file_size {
            return Ok(0);
        }
        let end = (self.pos + buf.len() as u64).min(self.file_size);
        let len = (end - self.pos) as usize;
        let mut written = 0;
        while written < len {
            let block_idx = self.pos / BLOCK_SIZE;
            self.fetch_block(block_idx).map_err(io::Error::other)?;
            let block = &self.cache[&block_idx];
            let off = (self.pos % BLOCK_SIZE) as usize;
            let avail = block.len().saturating_sub(off);
            let copy = (len - written).min(avail);
            buf[written..written + copy].copy_from_slice(&block[off..off + copy]);
            written += copy;
            self.pos += copy as u64;
        }
        Ok(len)
    }
}

impl Seek for RemoteFile {
    fn seek(&mut self, style: SeekFrom) -> io::Result<u64> {
        self.pos = match style {
            SeekFrom::Start(off) => off,
            SeekFrom::End(off) => (self.file_size as i64 + off) as u64,
            SeekFrom::Current(off) => (self.pos as i64 + off) as u64,
        };
        Ok(self.pos)
    }
}
