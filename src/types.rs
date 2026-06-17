use serde::Deserialize;
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, clap::ValueEnum)]
pub enum Backend {
    Wgc,
    Lgc,
    Cn360,
}

impl Backend {
    pub fn showroom_url(self, mirror: Option<&str>) -> &'static str {
        match (self, mirror) {
            (Backend::Wgc, Some("asia")) => "https://wguscs-wgcasia.wargaming.net/api/v20/content/showroom/?lang=EN&gameid=WGC.ASIA.PRODUCTION&format=json&wgc_publisher_id=wargaming&country_code=SG",
            (Backend::Wgc, Some("na")) => "https://wguscs-wgcna.wargaming.net/api/v20/content/showroom/?lang=EN&gameid=WGC.NA.PRODUCTION&format=json&wgc_publisher_id=wargaming&country_code=US",
            _ => match self {
                Backend::Wgc => "https://wguscs-wgceu.wargaming.net/api/v20/content/showroom/?lang=EN&gameid=WGC.EU.PRODUCTION&format=json&wgc_publisher_id=wargaming&country_code=US",
                Backend::Lgc => "https://lstuscs-ru.lesta.ru/api/v21/content/showroom/?lang=RU&gameid=LGC.RU.PRODUCTION&format=json&gc_publisher_id=lesta&country_code=RU",
                Backend::Cn360 => "https://wguscs-cn360.wggames.cn/api/v20/content/showroom/?lang=ZH_CN&gameid=WGC360.CN.PRODUCTION&format=json&wgc_publisher_id=qihoo&country_code=CN",
            },
        }
    }

    pub fn lang_code(self) -> &'static str {
        match self { Backend::Wgc => "EN", Backend::Lgc => "RU", Backend::Cn360 => "ZH_CN" }
    }

    pub fn gc_publisher(self) -> &'static str {
        match self { Backend::Wgc => "wargaming", Backend::Lgc => "lesta", Backend::Cn360 => "qihoo" }
    }

    pub fn gc_publisher_param(self) -> &'static str {
        match self { Backend::Wgc => "wgc_publisher_id", Backend::Lgc => "gc_publisher_id", Backend::Cn360 => "wgc_publisher_id" }
    }

    pub fn mirrors(self) -> &'static [&'static str] {
        match self {
            Backend::Wgc => &["asia", "na"],
            Backend::Lgc => &[],
            Backend::Cn360 => &[],
        }
    }

    pub fn resolve_mirror(self, name: &str) -> Result<(), String> {
        if self.mirrors().iter().any(|n| name.eq_ignore_ascii_case(n)) {
            Ok(())
        } else {
            let available = self.mirrors();
            if available.is_empty() {
                Err(format!("Backend '{:?}' has no mirrors available", self))
            } else {
                Err(format!("Unknown mirror '{}'. Available mirrors for '{:?}': {}", name, self, available.join(", ")))
            }
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct ShowroomResponse {
    pub data: ShowroomData,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ShowroomData {
    pub showcase: Vec<ShowcaseEntry>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ShowcaseEntry {
    pub game_name: Option<String>,
    pub instances: Vec<Instance>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Instance {
    pub application_id: Option<String>,
    pub update_service_url: Option<String>,
    pub name: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Game {
    pub app_id: String,
    pub api_base: String,
    pub game_name: String,
    pub region_name: String,
}

#[derive(Debug, Clone)]
pub struct Manifest {
    pub latest_version: Option<String>,
    pub metadata_version: String,
    pub chain_id: String,
    pub patches: HashMap<String, PatchPart>,
}

#[derive(Debug, Clone)]
pub struct PatchPart {
    pub part: String,
    pub version_from: String,
    pub version_to: String,
    pub files: Vec<FileEntry>,
}

#[derive(Debug, Clone)]
pub struct FileEntry {
    pub name: String,
    pub basename: String,
    pub size: u64,
    pub unpacked_size: u64,
    pub download_url: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ArchiveEntry {
    pub filename: String,
    pub offset: u64,
    pub compressed_size: u64,
    pub uncompressed_size: u64,
}
