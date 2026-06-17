use sevenz_rust2::{ArchiveReader, Password};

use crate::remote_file::RemoteFile;
use crate::types::ArchiveEntry;

/// Parses the 7z archive header from a remote file via range requests.
/// Returns file entries with calculated byte offsets for direct access.
pub fn parse_archive_index(rf: &mut RemoteFile) -> anyhow::Result<Vec<ArchiveEntry>> {
    let archive = sevenz_rust2::Archive::read(rf, &Password::empty())?;

    let sig_size: u64 = 32;
    let pack_pos = archive.pack_pos();
    let pack_sizes = archive.pack_sizes();
    let stream_map = &archive.stream_map;
    let pack_stream_offsets = stream_map.pack_stream_offsets();
    let block_pack_stream_idx = stream_map.block_first_pack_stream_index();

    let mut entries = Vec::new();
    for (file_idx, file) in archive.files.iter().enumerate() {
        if file.is_directory || !file.has_stream {
            continue;
        }
        if let Some(block_idx) = stream_map.file_block_index.get(file_idx).copied().flatten() {
            let stream_idx = block_pack_stream_idx
                .get(block_idx)
                .copied()
                .unwrap_or(0);
            let stream_offset = pack_stream_offsets
                .get(stream_idx)
                .copied()
                .unwrap_or(0);
            let csize = pack_sizes.get(stream_idx).copied().unwrap_or(0);
            let abs_offset = sig_size + pack_pos + stream_offset;

            entries.push(ArchiveEntry {
                filename: file.name.clone(),
                offset: abs_offset,
                compressed_size: csize,
                uncompressed_size: file.size,
            });
        }
    }

    Ok(entries)
}

/// Reads and decompresses a specific file from the remote 7z archive.
/// Each call re-parses the archive header, but RemoteFile's block cache
/// means the ~512KB header is only fetched once.
pub fn extract_entry(rf: &mut RemoteFile, name: &str) -> anyhow::Result<Vec<u8>> {
    let mut reader = ArchiveReader::new(rf, Password::empty())?;
    let data = reader
        .read_file(name)
        .map_err(|e| anyhow::anyhow!("Failed to extract '{}': {}", name, e))?;
    Ok(data)
}

/// Returns the number of non-directory, non-empty entries in the archive.
pub fn entry_count(rf: &mut RemoteFile) -> anyhow::Result<usize> {
    let archive = sevenz_rust2::Archive::read(rf, &Password::empty())?;
    Ok(archive
        .files
        .iter()
        .filter(|e| !e.is_directory && e.has_stream)
        .count())
}
