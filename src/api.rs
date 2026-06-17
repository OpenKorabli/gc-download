use std::collections::HashMap;

use anyhow::Context;
use roxmltree::Document;

use crate::types::*;

const CHAIN_BOOTSTRAP: &str = "f00";
const META_PROTO: &str = "7.10";
const PATCHES_PROTO: &str = "1.11";
const USER_AGENT: &str = "gc-download/0.1.0";

fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .build()
        .expect("Failed to build HTTP client")
}

pub async fn fetch_showroom(backend: Backend, mirror: Option<&str>) -> anyhow::Result<Vec<Game>> {
    let resp = client().get(backend.showroom_url(mirror)).send().await?;
    let sr: ShowroomResponse = resp.json().await?;
    let mut games = Vec::new();
    for entry in sr.data.showcase {
        let gname = entry.game_name.unwrap_or_default();
        for inst in entry.instances {
            let app_id = inst.application_id.unwrap_or_default();
            let update_url = inst.update_service_url.unwrap_or_default().trim_end_matches('/').to_string();
            let region = inst.name.unwrap_or_default();
            if !app_id.is_empty() && !update_url.is_empty() {
                games.push(Game {
                    api_base: format!("{}/api/v1", update_url),
                    app_id,
                    game_name: gname.clone(),
                    region_name: region,
                });
            }
        }
    }
    Ok(games)
}

pub async fn resolve_game(backend: Backend, app_id: &str, mirror: Option<&str>) -> anyhow::Result<Game> {
    let games = fetch_showroom(backend, mirror).await?;
    for g in &games {
        if g.app_id.eq_ignore_ascii_case(app_id) {
            return Ok(g.clone());
        }
    }
    anyhow::bail!("Unknown game '{}'. Use 'gc-download games' to list available games.", app_id)
}

pub async fn get_manifest(backend: Backend, api_base: &str, guid: &str) -> anyhow::Result<Manifest> {
    let metadata_xml = fetch_metadata(api_base, guid).await?;
    let meta = roxmltree::Document::parse(&metadata_xml)?;

    let metadata_version = text(&meta, "version").context("Missing version in metadata")?;
    let chain_id = nested_text(&meta, &["predefined_section", "chain_id"])
        .context("Missing chain_id in metadata")?;

    let client_types = meta
        .descendants()
        .find(|n| n.has_tag_name("client_types"))
        .context("Missing client_types in metadata")?;
    let default_ct = client_types
        .attribute("default")
        .context("client_types missing default attribute")?;

    let part_ids = get_part_ids(&client_types, default_ct)?;

    let patches_xml = fetch_patches_chain(backend, api_base, guid, &metadata_version, &chain_id, default_ct, &part_ids).await?;
    let patches_doc = roxmltree::Document::parse(&patches_xml)?;

    let mut patches = HashMap::new();
    let mut latest_version: Option<String> = None;

    if let Some(inner) = patches_doc.root_element().children().find(|n| n.has_tag_name("patches_chain")) {
        for patch in inner.children().filter(|n| n.has_tag_name("patch")) {
            let part = patch_text(&patch, "part").unwrap_or_default();
            let version_from = patch_text(&patch, "version_from").unwrap_or_default();
            let version_to = patch_text(&patch, "version_to").unwrap_or_default();

            if latest_version.is_none() && !version_to.is_empty() {
                latest_version = Some(version_to.clone());
            }

            let torrent_url = patch
                .descendants()
                .find(|n| n.has_tag_name("url"))
                .and_then(|n| n.text())
                .map(|s| s.trim().to_string())
                .unwrap_or_default();

            let mut files = Vec::new();
            if let Some(files_elem) = patch.children().find(|n| n.has_tag_name("files")) {
                for file_elem in files_elem.children().filter(|n| n.has_tag_name("file")) {
                    let name = patch_text(&file_elem, "name").unwrap_or_default();
                    let size: u64 = patch_text(&file_elem, "size").unwrap_or_default().parse().unwrap_or(0);
                    let unpacked: u64 = patch_text(&file_elem, "unpacked_size").unwrap_or_default().parse().unwrap_or(0);
                    let basename = std::path::Path::new(&name)
                        .file_name()
                        .map(|s| s.to_string_lossy().to_string())
                        .unwrap_or_default();
                    let download_url = if !torrent_url.is_empty() && !name.is_empty() {
                        build_direct_url(&torrent_url, &name)
                    } else {
                        None
                    };
                    files.push(FileEntry { name, basename, size, unpacked_size: unpacked, download_url });
                }
            }

            patches.insert(part.clone(), PatchPart { part, version_from, version_to, files });
        }
    }

    Ok(Manifest { latest_version, metadata_version, chain_id, patches })
}

fn get_part_ids(client_types: &roxmltree::Node, default_ct: &str) -> anyhow::Result<Vec<String>> {
    for ct in client_types.children().filter(|n| n.has_tag_name("client_type")) {
        if ct.attribute("id") == Some(default_ct) {
            if let Some(cp) = ct.descendants().find(|n| n.has_tag_name("client_parts")) {
                let ids: Vec<String> = cp.children()
                    .filter(|n| n.has_tag_name("client_part"))
                    .filter_map(|n| n.attribute("id").map(String::from))
                    .collect();
                if !ids.is_empty() {
                    return Ok(ids);
                }
            }
            break;
        }
    }
    anyhow::bail!("No client_parts found for client_type '{}'", default_ct)
}

fn build_direct_url(torrent_url: &str, file_name: &str) -> Option<String> {
    if torrent_url.is_empty() || file_name.is_empty() {
        return None;
    }
    let mut u = url::Url::parse(torrent_url).ok()?;
    let basename = std::path::Path::new(file_name)
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    let mut segs: Vec<String> = u.path_segments().map(|s| s.map(String::from).collect()).unwrap_or_default();
    if !segs.is_empty() {
        segs.pop();
        segs.push(basename);
    }
    u.set_path(&segs.join("/"));
    Some(u.to_string())
}

fn text(doc: &Document, tag: &str) -> Option<String> {
    doc.descendants()
        .find(|n| n.has_tag_name(tag))
        .and_then(|n| n.text())
        .map(|s| s.trim().to_string())
}

fn nested_text(doc: &Document, tags: &[&str]) -> Option<String> {
    let mut node = doc.root_element();
    for &t in tags {
        node = node.children().find(|n| n.has_tag_name(t))?;
    }
    node.text().map(|s| s.trim().to_string())
}

fn patch_text(node: &roxmltree::Node, tag: &str) -> Option<String> {
    node.children()
        .find(|n| n.has_tag_name(tag))
        .and_then(|n| n.text())
        .map(|s| s.trim().to_string())
}

async fn fetch_metadata(api_base: &str, guid: &str) -> anyhow::Result<String> {
    let url = format!(
        "{}/metadata/?guid={}&chain_id={}&protocol_version={}",
        api_base, guid, CHAIN_BOOTSTRAP, META_PROTO
    );
    let resp = client().get(&url).send().await?;
    resp.text().await.context("Failed to fetch metadata")
}

async fn fetch_patches_chain(
    backend: Backend,
    api_base: &str,
    guid: &str,
    metadata_version: &str,
    chain_id: &str,
    client_type: &str,
    part_ids: &[String],
) -> anyhow::Result<String> {
    let base_url = format!("{}/patches_chain/", api_base);
    let mut url = url::Url::parse(&base_url)?;
    {
        let mut pairs = url.query_pairs_mut();
        pairs.append_pair("game_id", guid);
        pairs.append_pair("protocol_version", PATCHES_PROTO);
        pairs.append_pair("metadata_version", metadata_version);
        pairs.append_pair("metadata_protocol_version", META_PROTO);
        pairs.append_pair("client_type", client_type);
        pairs.append_pair("lang", backend.lang_code());
        pairs.append_pair("chain_id", chain_id);
        pairs.append_pair("game_installation", "false");
        pairs.append_pair(backend.gc_publisher_param(), backend.gc_publisher());
        for pid in part_ids {
            pairs.append_pair(&format!("{}_current_version", pid), "0");
        }
    }
    let client = client();
    let resp = client.get(url).send().await?;
    resp.text().await.context("Failed to fetch patches chain")
}
