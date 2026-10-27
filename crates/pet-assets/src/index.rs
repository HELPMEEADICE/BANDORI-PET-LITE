//! One-shot model index: scan `models/*.zst`, cache the result, invalidate by
//! `size` + `mtime` (spec §4.2).

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use pet_config::ModelFormat;
use serde::{Deserialize, Serialize};
use tracing::{debug, info, warn};

use crate::{ARCHIVE_EXTENSION, ARCHIVE_INDEX_MEMBER, MODEL_INDEX_FILE};

/// Costumes discovered inside one archive.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArchiveScan {
    /// Character key, derived from the archive file stem.
    pub character: String,
    /// Costumes in archive order.
    pub costumes: Vec<CostumeEntry>,
}

/// One costume of one character.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CostumeEntry {
    /// Costume directory name, e.g. `live_default`.
    pub id: String,
    /// Virtual path for `config.models[i].path`.
    pub path: String,
    /// Container format of this costume.
    pub format: ModelFormat,
}

/// Scan cache entry for a single archive.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArchiveEntry {
    /// Archive size in bytes.
    pub size: u64,
    /// Modification time in nanoseconds since the Unix epoch.
    pub mtime_ns: u128,
    /// Discovered contents; `None` when the archive could not be read.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<ArchiveScan>,
}

/// `models/.bandori_model_index.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelIndex {
    /// Index format version.
    pub version: u32,
    /// Archive key (`models/kasumi.zst`) → scan entry.
    pub archives: BTreeMap<String, ArchiveEntry>,
}

impl Default for ModelIndex {
    fn default() -> Self {
        Self {
            version: 1,
            archives: BTreeMap::new(),
        }
    }
}

impl ModelIndex {
    /// Iterates `(archive key, character, costume)` for every usable costume.
    pub fn costumes(&self) -> impl Iterator<Item = (&str, &ArchiveScan, &CostumeEntry)> {
        self.archives
            .iter()
            .filter_map(|(key, entry)| {
                entry
                    .result
                    .as_ref()
                    .map(|scan| (key.as_str(), scan, scan.costumes.as_slice()))
            })
            .flat_map(|(key, scan, costumes)| {
                costumes.iter().map(move |costume| (key, scan, costume))
            })
    }

    /// Character keys sorted alphabetically.
    pub fn characters(&self) -> Vec<String> {
        let mut characters: BTreeSet<String> = BTreeSet::new();
        for (_, scan, _) in self.costumes() {
            characters.insert(scan.character.clone());
        }
        characters.into_iter().collect()
    }

    /// Costumes of one character.
    pub fn costumes_of(&self, character: &str) -> Vec<CostumeEntry> {
        self.costumes()
            .filter(|(_, scan, _)| scan.character == character)
            .map(|(_, _, costume)| costume.clone())
            .collect()
    }

    /// Finds one costume by character and costume id.
    pub fn find(&self, character: &str, costume: &str) -> Option<CostumeEntry> {
        self.costumes()
            .find(|(_, scan, entry)| scan.character == character && entry.id == costume)
            .map(|(_, _, entry)| entry.clone())
    }

    /// Total number of costumes.
    pub fn costume_count(&self) -> usize {
        self.costumes().count()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ModelIndexError {
    /// Archive missing or unreadable.
    #[error("failed to read archive {path}: {source}")]
    Archive {
        /// Archive path.
        path: PathBuf,
        /// Underlying error.
        #[source]
        source: std::io::Error,
    },
    /// zstd stream failure.
    #[error("zstd error in {path}: {source}")]
    Zstd {
        /// Archive path.
        path: PathBuf,
        /// Underlying error.
        #[source]
        source: std::io::Error,
    },
    /// Tar stream failure.
    #[error("tar error in {path}: {source}")]
    Tar {
        /// Archive path.
        path: PathBuf,
        /// Underlying error.
        #[source]
        source: std::io::Error,
    },
    /// No model.json / *.model3.json was found in the archive.
    #[error("archive {path} contains no Cubism model")]
    NoModel {
        /// Archive path.
        path: PathBuf,
    },
}

/// Scans every `*.zst` under `models_dir`, reusing cached entries.
pub fn load_or_scan(models_dir: &Path) -> ModelIndex {
    let cache_path = models_dir.join(MODEL_INDEX_FILE);
    let mut cached = read_cached_index(&cache_path);
    let root = models_dir.parent().unwrap_or(models_dir);

    let mut archives_on_disk = BTreeSet::new();
    match fs::read_dir(models_dir) {
        Ok(entries) => {
            for entry in entries.flatten() {
                let path = entry.path();
                let is_archive = path
                    .extension()
                    .map(|extension| extension.eq_ignore_ascii_case(ARCHIVE_EXTENSION))
                    .unwrap_or(false);
                if !is_archive {
                    continue;
                }
                let Some(key) = archive_key(root, &path) else {
                    continue;
                };
                archives_on_disk.insert(key.clone());
                let (size, mtime_ns) = match stat(&path) {
                    Some(stamp) => stamp,
                    None => continue,
                };
                let fresh = cached.archives.get(&key).is_some_and(|entry| {
                    entry.size == size && entry.mtime_ns == mtime_ns && entry.result.is_some()
                });
                if fresh {
                    continue;
                }
                debug!(archive = %path.display(), "scanning model archive");
                match scan_archive(&path) {
                    Ok(scan) => {
                        cached.archives.insert(
                            key,
                            ArchiveEntry {
                                size,
                                mtime_ns,
                                result: Some(scan),
                            },
                        );
                    }
                    Err(error) => {
                        warn!(archive = %path.display(), %error, "model archive scan failed");
                        cached.archives.insert(
                            key,
                            ArchiveEntry {
                                size,
                                mtime_ns,
                                result: None,
                            },
                        );
                    }
                }
            }
        }
        Err(error) => {
            warn!(dir = %models_dir.display(), %error, "models directory unreadable");
        }
    }

    cached
        .archives
        .retain(|key, _| archives_on_disk.contains(key));
    cached.version = 1;

    if cached != read_cached_index(&cache_path) {
        if let Err(error) = write_index(&cache_path, &cached) {
            warn!(path = %cache_path.display(), %error, "failed to write model index cache");
        }
    }
    info!(
        archives = cached.archives.len(),
        costumes = cached.costume_count(),
        "model index ready"
    );
    cached
}

fn read_cached_index(path: &Path) -> ModelIndex {
    match fs::read_to_string(path) {
        Ok(raw) => serde_json::from_str(&raw).unwrap_or_else(|error| {
            warn!(path = %path.display(), %error, "model index cache unparsable, rescanning");
            ModelIndex::default()
        }),
        Err(_) => ModelIndex::default(),
    }
}

fn write_index(path: &Path, index: &ModelIndex) -> std::io::Result<()> {
    let bytes = serde_json::to_vec(index).map_err(std::io::Error::other)?;
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, bytes)?;
    fs::rename(tmp, path)
}

/// Cache key for an archive: its path relative to the installation root with
/// forward slashes (`models/kasumi.zst`).
pub fn archive_key(root: &Path, archive: &Path) -> Option<String> {
    let relative = archive.strip_prefix(root).ok()?;
    let mut parts = Vec::new();
    for component in relative.components() {
        parts.push(component.as_os_str().to_string_lossy().to_string());
    }
    if parts.is_empty() {
        return None;
    }
    Some(parts.join("/"))
}

fn stat(path: &Path) -> Option<(u64, u128)> {
    let metadata = fs::metadata(path).ok()?;
    let mtime = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    Some((metadata.len(), mtime))
}

/// Reads one archive and derives its character + costume list.
pub fn scan_archive(archive: &Path) -> Result<ArchiveScan, ModelIndexError> {
    let stem = archive
        .file_stem()
        .map(|stem| stem.to_string_lossy().to_string())
        .unwrap_or_default();
    let file_prefix = archive
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_default();

    let members = list_members(archive)?;
    let mut costumes: BTreeMap<String, ModelFormat> = BTreeMap::new();
    for member in &members {
        let mut parts = member.split('/');
        let Some(directory) = parts.next() else {
            continue;
        };
        let Some(file) = parts.next() else {
            continue;
        };
        if parts.next().is_some() || directory.is_empty() {
            continue;
        }
        let lower = file.to_ascii_lowercase();
        let format = if lower.ends_with(".model3.json") {
            ModelFormat::Moc3
        } else if lower == "model.json" {
            ModelFormat::Moc
        } else {
            continue;
        };
        costumes
            .entry(directory.to_owned())
            .and_modify(|existing| {
                if format == ModelFormat::Moc3 {
                    *existing = ModelFormat::Moc3;
                }
            })
            .or_insert(format);
    }

    if costumes.is_empty() {
        return Err(ModelIndexError::NoModel {
            path: archive.to_path_buf(),
        });
    }

    let costumes = costumes
        .into_iter()
        .map(|(id, format)| {
            let model_file = if format == ModelFormat::Moc3 {
                // Prefer the member we actually saw.
                members
                    .iter()
                    .find(|member| {
                        member.starts_with(&format!("{id}/"))
                            && member.to_ascii_lowercase().ends_with(".model3.json")
                    })
                    .and_then(|member| member.split_once('/').map(|(_, file)| file.to_owned()))
                    .unwrap_or_else(|| "model.json".to_owned())
            } else {
                "model.json".to_owned()
            };
            CostumeEntry {
                id: id.clone(),
                path: format!("models/{file_prefix}::{id}/{model_file}"),
                format,
            }
        })
        .collect();

    Ok(ArchiveScan {
        character: stem,
        costumes,
    })
}

/// Lists every regular-file member of an archive.
///
/// The archive's own `.bandori_zst_index.json` member holds the same list, so a
/// full decompression pass can be skipped as soon as that member is reached.
pub fn list_members(archive: &Path) -> Result<Vec<String>, ModelIndexError> {
    let mut decoder = open_decoder(archive)?;
    let mut archive_reader = tar::Archive::new(&mut decoder);
    let entries = archive_reader
        .entries()
        .map_err(|source| ModelIndexError::Tar {
            path: archive.to_path_buf(),
            source,
        })?;

    let mut members = Vec::new();
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(_) => break,
        };
        let path = match entry.path() {
            Ok(path) => path.to_string_lossy().replace('\\', "/"),
            Err(_) => continue,
        };
        let is_file = entry.header().entry_type().is_file();
        if path == ARCHIVE_INDEX_MEMBER && is_file {
            // The member list is authoritative and already complete.
            let mut bytes = Vec::new();
            let mut entry = entry;
            if entry.read_to_end(&mut bytes).is_ok() {
                if let Ok(list) = serde_json::from_slice::<ArchiveMemberList>(&bytes) {
                    if !list.files.is_empty() {
                        return Ok(list.files);
                    }
                }
            }
            continue;
        }
        if is_file {
            members.push(path);
        }
    }
    Ok(members)
}

#[derive(Debug, Deserialize)]
struct ArchiveMemberList {
    #[serde(default)]
    files: Vec<String>,
}

fn open_decoder(
    archive: &Path,
) -> Result<zstd::stream::read::Decoder<'static, std::io::BufReader<fs::File>>, ModelIndexError> {
    let file = fs::File::open(archive).map_err(|source| ModelIndexError::Archive {
        path: archive.to_path_buf(),
        source,
    })?;
    zstd::stream::read::Decoder::new(file).map_err(|source| ModelIndexError::Zstd {
        path: archive.to_path_buf(),
        source,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// Builds a tiny `.zst` tar containing an index member plus two costumes.
    fn write_archive(
        dir: &Path,
        stem: &str,
        members: &[(&str, &str)],
        index: Option<&str>,
    ) -> PathBuf {
        let archive_path = dir.join(format!("{stem}.zst"));
        let file = fs::File::create(&archive_path).unwrap();
        let encoder = zstd::stream::write::Encoder::new(file, 3).unwrap();
        let mut builder = tar::Builder::new(encoder);
        if let Some(index_json) = index {
            let mut header = tar::Header::new_gnu();
            header.set_size(index_json.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            builder
                .append_data(&mut header, ARCHIVE_INDEX_MEMBER, index_json.as_bytes())
                .unwrap();
        }
        for (name, contents) in members {
            let mut header = tar::Header::new_gnu();
            header.set_size(contents.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            builder
                .append_data(&mut header, *name, contents.as_bytes())
                .unwrap();
        }
        let encoder = builder.into_inner().unwrap();
        let mut file = encoder.finish().unwrap();
        file.flush().unwrap();
        archive_path
    }

    #[test]
    fn derives_costumes_from_directory_layout() {
        let dir = tempfile::tempdir().unwrap();
        let archive = write_archive(
            dir.path(),
            "kasumi",
            &[
                ("live_default/model.json", "{}"),
                ("live_default/data/model.moc", "x"),
                ("casual/adv_live2d_kasumi_casual.model3.json", "{}"),
            ],
            None,
        );
        let scan = scan_archive(&archive).unwrap();
        assert_eq!(scan.character, "kasumi");
        assert_eq!(scan.costumes.len(), 2);
        let live = scan
            .costumes
            .iter()
            .find(|c| c.id == "live_default")
            .unwrap();
        assert_eq!(live.format, ModelFormat::Moc);
        assert_eq!(live.path, "models/kasumi.zst::live_default/model.json");
        let casual = scan.costumes.iter().find(|c| c.id == "casual").unwrap();
        assert_eq!(casual.format, ModelFormat::Moc3);
        assert_eq!(
            casual.path,
            "models/kasumi.zst::casual/adv_live2d_kasumi_casual.model3.json"
        );
    }

    #[test]
    fn uses_embedded_member_list_when_present() {
        let dir = tempfile::tempdir().unwrap();
        let archive = write_archive(
            dir.path(),
            "anon",
            &[("casual/model.json", "{}")],
            Some(r#"{"files":["casual/model.json","character.png"]}"#),
        );
        let members = list_members(&archive).unwrap();
        assert_eq!(members, vec!["casual/model.json", "character.png"]);
    }

    #[test]
    fn index_round_trips_and_filters_removed_archives() {
        let dir = tempfile::tempdir().unwrap();
        let models = dir.path().join("models");
        fs::create_dir_all(&models).unwrap();
        write_archive(
            &models,
            "kasumi",
            &[("live_default/model.json", "{}")],
            None,
        );
        let index = load_or_scan(&models);
        assert_eq!(index.archives.len(), 1);
        assert!(index.archives.contains_key("models/kasumi.zst"));
        assert_eq!(index.costumes_of("kasumi").len(), 1);

        fs::remove_file(models.join("kasumi.zst")).unwrap();
        let index = load_or_scan(&models);
        assert!(index.archives.is_empty());
    }

    #[test]
    fn scan_is_cached_when_archive_is_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        let models = dir.path().join("models");
        fs::create_dir_all(&models).unwrap();
        write_archive(
            &models,
            "kasumi",
            &[("live_default/model.json", "{}")],
            None,
        );
        let first = load_or_scan(&models);
        let cache = read_cached_index(&models.join(MODEL_INDEX_FILE));
        assert_eq!(first, cache);
        let second = load_or_scan(&models);
        assert_eq!(first, second);
    }
}
