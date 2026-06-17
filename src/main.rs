use std::path::Path;

use anyhow::Context;
use clap::{Parser, Subcommand};
use futures::StreamExt;
use indicatif::{ProgressBar, ProgressStyle};
use gc_download::api::{fetch_showroom, get_manifest, resolve_game};
use gc_download::remote_file::RemoteFile;
use gc_download::sevenz::{extract_entry, parse_archive_index};
use gc_download::types::{Backend, FileEntry};
use tokio::io::AsyncWriteExt;

#[derive(Parser)]
#[command(name = "gc-download", about = "Download game files from WGC / LGC API")]
struct Cli {
    /// Backend: Wargaming Game Center (wgc) or Lesta Game Center (lgc)
    #[arg(short = 'b', long, default_value = "lgc", value_enum)]
    backend: Backend,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// List all available games
    Games,
    /// List versions, parts, and files for a game
    List {
        /// Game ID (e.g. MK.RU.PRODUCTION, WOT.EU.PRODUCTION)
        game: String,
        /// Show files in a specific part
        #[arg(long)]
        files: Option<String>,
        /// Show all parts (default overview already includes them)
        #[arg(long)]
        parts: bool,
    },
    /// Download file(s) from a part
    Download {
        /// Game ID
        game: String,
        /// Part name (e.g. hotfix, client, locale)
        part: String,
        /// File to download (basename or full name)
        filename: Option<String>,
        /// Output path (default: basename)
        #[arg(short = 'o', long)]
        output: Option<String>,
        /// Output directory when using --all
        #[arg(short = 'd', long)]
        dir: Option<String>,
        /// Download all files in the part
        #[arg(long)]
        all: bool,
    },
    /// Extract files from a remote .dspkg archive (no full download needed)
    Extract {
        /// Game ID
        game: String,
        /// Part name (e.g. client, locale)
        part: String,
        /// Specific file paths to extract (supports globs)
        paths: Vec<String>,
        /// Output directory
        #[arg(short = 'd', long, default_value = ".")]
        dir: String,
        /// Only list files in the archive, don't extract
        #[arg(long)]
        list: bool,
        /// Extract files matching this glob pattern
        #[arg(long)]
        filter: Option<String>,
    },
}

fn fmt_size(n: u64) -> String {
    const UNITS: &[&str] = &["B", "KB", "MB", "GB", "TB"];
    let mut size = n as f64;
    for unit in UNITS {
        if size < 1024.0 {
            return format!("{:.1} {}", size, unit);
        }
        size /= 1024.0;
    }
    format!("{:.1} TB", size)
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Games => cmd_games(cli.backend).await,
        Commands::List { game, files, .. } => cmd_list(cli.backend, &game, files.as_deref()).await,
        Commands::Download { game, part, filename, output, dir, all } => {
            cmd_download(cli.backend, &game, &part, filename.as_deref(), output.as_deref(), dir.as_deref(), all).await
        }
        Commands::Extract { game, part, paths, dir, list: do_list, filter } => {
            cmd_extract(cli.backend, &game, &part, &paths, &dir, do_list, filter.as_deref()).await
        }
    }
}

async fn cmd_games(backend: Backend) -> anyhow::Result<()> {
    let games = fetch_showroom(backend).await?;
    println!("Available games:\n");
    for g in &games {
        println!("  {:<25} {:<25} {}", g.app_id, g.game_name, g.region_name);
    }
    Ok(())
}

async fn cmd_list(backend: Backend, game_id: &str, files_part: Option<&str>) -> anyhow::Result<()> {
    let game = resolve_game(backend, game_id).await?;
    let manifest = get_manifest(backend, &game.api_base, &game.app_id).await?;

    if let Some(part_name) = files_part {
        let part = manifest
            .patches
            .get(part_name)
            .ok_or_else(|| anyhow::anyhow!("Part '{}' not found. Available: {}", part_name, manifest.patches.keys().cloned().collect::<Vec<_>>().join(", ")))?;

        println!("Part: {}", part.part);
        println!("Version: {} -> {}", part.version_from, part.version_to);
        println!("Files ({}):\n", part.files.len());
        for f in &part.files {
            println!("  {:<50} {:>10}  (unpacked: {})", f.basename, fmt_size(f.size), fmt_size(f.unpacked_size));
        }
        return Ok(());
    }

    println!("Game:              {}  ({} — {})", game.app_id, game.game_name, game.region_name);
    println!("Latest version:    {}", manifest.latest_version.as_deref().unwrap_or("?"));
    println!("Metadata version:  {}", manifest.metadata_version);
    println!("Chain ID:          {}", manifest.chain_id);
    println!("\nParts ({}):\n", manifest.patches.len());

    for (name, part) in &manifest.patches {
        let total_size: u64 = part.files.iter().map(|f| f.size).sum();
        println!(
            "  {:<20} {:>3} file(s)   {:>10}   [{} -> {}]",
            name,
            part.files.len(),
            fmt_size(total_size),
            part.version_from,
            part.version_to
        );
    }

    println!("\nUse 'list <GAME> --files <PART>' to see individual files.");
    Ok(())
}

async fn cmd_download(
    backend: Backend,
    game_id: &str,
    part_name: &str,
    filename: Option<&str>,
    output: Option<&str>,
    dir: Option<&str>,
    all: bool,
) -> anyhow::Result<()> {
    let game = resolve_game(backend, game_id).await?;
    let manifest = get_manifest(backend, &game.api_base, &game.app_id).await?;

    let part = manifest
        .patches
        .get(part_name)
        .ok_or_else(|| anyhow::anyhow!("Part '{}' not found.", part_name))?;

    if all {
        let out_dir = dir.unwrap_or(".");
        tokio::fs::create_dir_all(out_dir).await?;
        for f in &part.files {
            download_file(f, &Path::new(out_dir).join(&f.basename)).await?;
        }
    } else {
        let fname = filename.ok_or_else(|| anyhow::anyhow!("Specify a FILENAME or use --all"))?;
        let match_file = part
            .files
            .iter()
            .find(|f| f.basename == fname || f.name == fname)
            .ok_or_else(|| {
                let files: Vec<_> = part.files.iter().map(|f| f.basename.as_str()).collect();
                anyhow::anyhow!("File '{}' not found. Available: {:?}", fname, files)
            })?;
        let target = output.map(|o| o.to_string()).unwrap_or_else(|| match_file.basename.clone());
        download_file(match_file, Path::new(&target)).await?;
    }
    Ok(())
}

async fn download_file(file: &FileEntry, target: &Path) -> anyhow::Result<()> {
    let url = file.download_url.as_deref().ok_or_else(|| anyhow::anyhow!("No download URL for {}", file.basename))?;
    let expected = file.size;

    if target.exists() && target.metadata().map(|m| m.len() == expected).unwrap_or(false) {
        println!("  SKIP {} (already downloaded, {})", file.basename, fmt_size(expected));
        return Ok(());
    }

    let tmp = target.with_extension("part");
    let client = reqwest::Client::builder()
        .user_agent("gc-download/0.1.0")
        .build()?;

    let resp = client.get(url).send().await?;
    let total = resp.content_length().unwrap_or(expected);

    let pb = ProgressBar::new(total);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("{msg} [{bar:40}] {bytes}/{total_bytes} ({eta})")?
            .progress_chars("=> "),
    );
    pb.set_message(format!("  GET {}", file.basename));

    let mut file_handle = tokio::fs::File::create(&tmp).await?;
    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        file_handle.write_all(&chunk).await?;
        pb.inc(chunk.len() as u64);
    }
    pb.finish_and_clear();

    tokio::fs::rename(&tmp, target).await?;
    println!("  OK   {} -> {}", file.basename, target.display());
    Ok(())
}

async fn cmd_extract(
    backend: Backend,
    game_id: &str,
    part_name: &str,
    paths: &[String],
    out_dir: &str,
    do_list: bool,
    filter: Option<&str>,
) -> anyhow::Result<()> {
    let game = resolve_game(backend, game_id).await?;
    let manifest = get_manifest(backend, &game.api_base, &game.app_id).await?;

    let part = manifest
        .patches
        .get(part_name)
        .ok_or_else(|| anyhow::anyhow!("Part '{}' not found.", part_name))?;

    let first_file = part
        .files
        .first()
        .ok_or_else(|| anyhow::anyhow!("No files in part '{}'", part_name))?;

    let url = first_file
        .download_url
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("No download URL for this part"))?;

    println!("Archive: {} ({})", first_file.basename, fmt_size(first_file.size));
    println!("URL: {}", url);
    println!("Reading archive index via range requests...");

    let mut rf = RemoteFile::new(url).context("Failed to open remote file")?;
    let entries = parse_archive_index(&mut rf)?;
    let files_only: Vec<_> = entries.iter().filter(|e| e.compressed_size > 0).collect();

    println!(
        "Index: {} files ({} HTTP requests)",
        files_only.len(),
        rf.requests
    );

    if do_list {
        for e in &files_only {
            println!(
                "  {:<70} {:>10}  (compressed: {})",
                e.filename,
                fmt_size(e.uncompressed_size),
                fmt_size(e.compressed_size)
            );
        }
        println!("\n{} files total", files_only.len());
        return Ok(());
    }

    let to_extract: Vec<&gc_download::types::ArchiveEntry> = if let Some(filt) = filter {
        files_only
            .into_iter()
            .filter(|e| {
                glob_match::glob_match(filt, &e.filename)
                    || glob_match::glob_match(filt, &format!("*/{}", e.filename))
            })
            .collect()
    } else if !paths.is_empty() {
        files_only
            .into_iter()
            .filter(|e| {
                paths
                    .iter()
                    .any(|p| e.filename == *p || e.filename.ends_with(&format!("/{}", p)))
            })
            .collect()
    } else {
        files_only.into_iter().collect()
    };

    if to_extract.is_empty() {
        anyhow::bail!("No files matched. Use --list to see available files.");
    }

    let total_c: u64 = to_extract.iter().map(|e| e.compressed_size).sum();
    let total_u: u64 = to_extract.iter().map(|e| e.uncompressed_size).sum();
    println!(
        "Extracting {} file(s): {} to download, {} uncompressed\n",
        to_extract.len(),
        fmt_size(total_c),
        fmt_size(total_u)
    );

    let mut extracted = 0;
    for entry in &to_extract {
        let rel_path = entry.filename.replace('/', std::path::MAIN_SEPARATOR_STR);
        let dest = Path::new(out_dir).join(&rel_path);

        if dest.exists() && dest.metadata().map(|m| m.len() == entry.uncompressed_size).unwrap_or(false) {
            println!("  SKIP {}", entry.filename);
            continue;
        }

        print!("  GET  {} ({}) ...", entry.filename, fmt_size(entry.compressed_size));
        std::io::Write::flush(&mut std::io::stdout())?;

        let data = extract_entry(&mut rf, &entry.filename)?;

        if let Some(parent) = dest.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        tokio::fs::write(&dest, &data).await?;
        extracted += 1;
        println!(" -> {} ({} req)", fmt_size(data.len() as u64), rf.requests);
    }

    println!("\nDone: {} file(s) extracted to {}", extracted, out_dir);
    println!("Total HTTP requests: {}", rf.requests);
    Ok(())
}
