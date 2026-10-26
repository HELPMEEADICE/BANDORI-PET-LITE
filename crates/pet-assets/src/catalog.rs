//! Join the model index with `band.json` / `outfit.json` display names.
//!
//! `band.json` groups characters into bands (12 + `others` in the shipping data),
//! `outfit.json` maps `<character>` → display name plus `<costume>` → Chinese name.
//! Only characters that actually ship a model archive are exposed.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use pet_config::ModelFormat;
use serde::Deserialize;
use tracing::warn;

use crate::index::ModelIndex;

/// One selectable character.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogCharacter {
    /// Config key (`kasumi`).
    pub key: String,
    /// Display name (`户山香澄`); falls back to the key.
    pub display: String,
    /// Band id (`poppin_party`), empty for characters outside `band.json`.
    pub band_id: String,
    /// Band display name (`Poppin'Party`).
    pub band_display: String,
    /// Costumes, sorted by id to match the reference picker.
    pub costumes: Vec<CatalogCostume>,
}

/// One selectable costume.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogCostume {
    /// Costume id (`live_event_295_ur`).
    pub id: String,
    /// Chinese display name, falling back to the id.
    pub display: String,
    /// Virtual path for `config.models[i].path`.
    pub path: String,
    /// Container format.
    pub format: ModelFormat,
}

impl CatalogCostume {
    /// `"<display> (<id>)"` for list rows.
    pub fn label(&self) -> String {
        if self.display == self.id {
            self.id.clone()
        } else {
            format!("{} ({})", self.display, self.id)
        }
    }
}

/// Characters and costumes available in an installation.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Catalog {
    characters: Vec<CatalogCharacter>,
    by_key: BTreeMap<String, usize>,
    bands: Vec<CatalogBand>,
}

/// A band with installed characters, in the order declared by `band.json`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogBand {
    pub id: String,
    pub display: String,
    pub logo: String,
    pub characters: Vec<String>,
}

/// A remotely available model package, including characters not yet installed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelPackage {
    pub key: String,
    pub display: String,
    pub band_display: String,
}

/// Download catalog in the same band/member order as the character picker.
/// Bundled metadata also makes this available in a fresh installation.
pub fn model_packages(assets_dir: &Path) -> Vec<ModelPackage> {
    let bands = read_bands(&assets_dir.join("band.json"));
    let mut outfits = read_outfits(&assets_dir.join("outfit.json"));
    let mut packages = Vec::new();
    for band in bands {
        for key in band.characters {
            if let Some(info) = outfits.remove(&key) {
                packages.push(ModelPackage {
                    display: if info.display.is_empty() {
                        key.clone()
                    } else {
                        info.display
                    },
                    key,
                    band_display: band.display.clone(),
                });
            }
        }
    }
    packages.extend(outfits.into_iter().map(|(key, info)| ModelPackage {
        display: if info.display.is_empty() {
            key.clone()
        } else {
            info.display
        },
        key,
        band_display: "其他角色".into(),
    }));
    packages
}

impl Catalog {
    /// Builds a catalog from the scanned index plus the two JSON metadata files.
    pub fn build(assets_dir: &Path, index: &ModelIndex) -> Self {
        let bands = read_bands(&assets_dir.join("band.json"));
        let outfits = read_outfits(&assets_dir.join("outfit.json"));

        let mut characters: BTreeMap<String, CatalogCharacter> = BTreeMap::new();
        for (_, scan, costume) in index.costumes() {
            let character =
                characters
                    .entry(scan.character.clone())
                    .or_insert_with(|| CatalogCharacter {
                        key: scan.character.clone(),
                        display: outfits
                            .get(&scan.character)
                            .map(|outfit| outfit.display.clone())
                            .filter(|display| !display.is_empty())
                            .unwrap_or_else(|| scan.character.clone()),
                        band_id: String::new(),
                        band_display: String::new(),
                        costumes: Vec::new(),
                    });
            let display = outfits
                .get(&scan.character)
                .and_then(|outfit| outfit.costumes.get(&costume.id).cloned())
                .filter(|name| !name.is_empty())
                .unwrap_or_else(|| costume.id.clone());
            character.costumes.push(CatalogCostume {
                id: costume.id.clone(),
                display,
                path: costume.path.clone(),
                format: costume.format,
            });
        }

        for band in &bands {
            for character_key in &band.characters {
                if let Some(character) = characters.get_mut(character_key) {
                    character.band_id = band.id.clone();
                    character.band_display = band.display.clone();
                }
            }
        }

        // Band order first (as declared by band.json), then unaffiliated keys.
        let order: BTreeMap<&str, usize> = bands
            .iter()
            .enumerate()
            .map(|(position, band)| (band.id.as_str(), position))
            .collect();
        let member_order: BTreeMap<&str, usize> = bands
            .iter()
            .flat_map(|band| {
                band.characters
                    .iter()
                    .enumerate()
                    .map(|(i, key)| (key.as_str(), i))
            })
            .collect();
        let mut characters: Vec<CatalogCharacter> = characters.into_values().collect();
        characters.sort_by(|left, right| {
            let left_rank = order
                .get(left.band_id.as_str())
                .copied()
                .unwrap_or(usize::MAX);
            let right_rank = order
                .get(right.band_id.as_str())
                .copied()
                .unwrap_or(usize::MAX);
            left_rank
                .cmp(&right_rank)
                .then_with(|| {
                    member_order
                        .get(left.key.as_str())
                        .cmp(&member_order.get(right.key.as_str()))
                })
                .then_with(|| left.display.cmp(&right.display))
                .then_with(|| left.key.cmp(&right.key))
        });
        for character in &mut characters {
            character
                .costumes
                .sort_by(|left, right| left.id.cmp(&right.id));
        }

        let mut catalog_bands: Vec<_> = bands
            .iter()
            .filter_map(|band| {
                let members: Vec<_> = band
                    .characters
                    .iter()
                    .filter(|key| characters.iter().any(|character| &character.key == *key))
                    .cloned()
                    .collect();
                (!members.is_empty()).then(|| CatalogBand {
                    id: band.id.clone(),
                    display: band.display.clone(),
                    logo: band.logo.clone(),
                    characters: members,
                })
            })
            .collect();
        let unaffiliated: Vec<_> = characters
            .iter()
            .filter(|character| character.band_id.is_empty())
            .map(|character| character.key.clone())
            .collect();
        if !unaffiliated.is_empty() {
            catalog_bands.push(CatalogBand {
                id: "__custom__".into(),
                display: "自定义角色".into(),
                logo: String::new(),
                characters: unaffiliated,
            });
        }
        let by_key = characters
            .iter()
            .enumerate()
            .map(|(position, character)| (character.key.clone(), position))
            .collect();
        Self {
            characters,
            by_key,
            bands: catalog_bands,
        }
    }

    /// All characters, in display order.
    pub fn characters(&self) -> &[CatalogCharacter] {
        &self.characters
    }

    pub fn bands(&self) -> &[CatalogBand] {
        &self.bands
    }

    /// Looks up one character.
    pub fn character(&self, key: &str) -> Option<&CatalogCharacter> {
        self.by_key
            .get(key)
            .map(|position| &self.characters[*position])
    }

    /// Costumes of one character.
    pub fn costumes_of(&self, key: &str) -> &[CatalogCostume] {
        self.character(key)
            .map(|character| character.costumes.as_slice())
            .unwrap_or(&[])
    }

    /// Finds one costume of one character.
    pub fn find(&self, character: &str, costume: &str) -> Option<&CatalogCostume> {
        self.costumes_of(character)
            .iter()
            .find(|entry| entry.id == costume)
    }

    /// Characters whose name or key contains `needle` (case-insensitive).
    pub fn search(&self, needle: &str) -> Vec<&CatalogCharacter> {
        let needle = needle.trim().to_lowercase();
        if needle.is_empty() {
            return self.characters.iter().collect();
        }
        self.characters
            .iter()
            .filter(|character| {
                character.key.to_lowercase().contains(&needle)
                    || character.display.to_lowercase().contains(&needle)
                    || character.band_display.to_lowercase().contains(&needle)
            })
            .collect()
    }

    /// Number of characters with at least one costume.
    pub fn len(&self) -> usize {
        self.characters.len()
    }

    /// `true` when no model archive was found.
    pub fn is_empty(&self) -> bool {
        self.characters.is_empty()
    }
}

#[derive(Debug, Deserialize)]
struct BandFile {
    #[serde(default)]
    bands: Vec<Band>,
}

#[derive(Debug, Deserialize)]
struct Band {
    #[serde(default)]
    id: String,
    #[serde(default)]
    display: String,
    #[serde(default)]
    logo: String,
    #[serde(default)]
    characters: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct OutfitFile {
    #[serde(default)]
    characters: BTreeMap<String, OutfitCharacter>,
}

#[derive(Debug, Deserialize)]
struct OutfitCharacter {
    #[serde(default)]
    display: String,
    #[serde(default)]
    costumes: BTreeMap<String, String>,
}

fn read_bands(path: &Path) -> Vec<Band> {
    match fs::read_to_string(path) {
        Ok(raw) => serde_json::from_str::<BandFile>(&raw)
            .map(|file| file.bands)
            .unwrap_or_else(|error| {
                warn!(path = %path.display(), %error, "band.json unparsable");
                bundled_bands()
            }),
        Err(error) => {
            tracing::debug!(path = %path.display(), %error, "using bundled band.json");
            bundled_bands()
        }
    }
}

fn read_outfits(path: &Path) -> BTreeMap<String, OutfitCharacter> {
    match fs::read_to_string(path) {
        Ok(raw) => serde_json::from_str::<OutfitFile>(&raw)
            .map(|file| file.characters)
            .unwrap_or_else(|error| {
                warn!(path = %path.display(), %error, "outfit.json unparsable");
                bundled_outfits()
            }),
        Err(error) => {
            tracing::debug!(path = %path.display(), %error, "using bundled outfit.json");
            bundled_outfits()
        }
    }
}

fn bundled_bands() -> Vec<Band> {
    serde_json::from_str::<BandFile>(include_str!("../../../assets/band.json"))
        .expect("bundled band.json must be valid")
        .bands
}

fn bundled_outfits() -> BTreeMap<String, OutfitCharacter> {
    serde_json::from_str::<OutfitFile>(include_str!("../../../assets/outfit.json"))
        .expect("bundled outfit.json must be valid")
        .characters
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::index::{ArchiveEntry, ArchiveScan, CostumeEntry};

    #[test]
    fn download_catalog_includes_uninstalled_characters_and_bundled_metadata() {
        let dir = tempfile::tempdir().unwrap();
        let packages = model_packages(dir.path());
        assert_eq!(packages.first().unwrap().key, "kasumi");
        assert!(packages.iter().any(|package| package.key == "anon"));
        let keys: std::collections::BTreeSet<_> =
            packages.iter().map(|package| &package.key).collect();
        assert_eq!(keys.len(), packages.len());
        assert!(packages.len() >= 40);
        assert!(Catalog::build(dir.path(), &ModelIndex::default()).is_empty());
    }

    fn index_with(character: &str, costumes: &[(&str, ModelFormat)]) -> ModelIndex {
        let mut index = ModelIndex::default();
        index.archives.insert(
            format!("models/{character}.zst"),
            ArchiveEntry {
                size: 1,
                mtime_ns: 0,
                result: Some(ArchiveScan {
                    character: character.to_owned(),
                    costumes: costumes
                        .iter()
                        .map(|(id, format)| CostumeEntry {
                            id: (*id).to_owned(),
                            path: format!("models/{character}.zst::{id}/model.json"),
                            format: *format,
                        })
                        .collect(),
                }),
            },
        );
        index
    }

    #[test]
    fn joins_display_names_and_band_order() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path()).unwrap();
        fs::write(
            dir.path().join("band.json"),
            r#"{"bands":[
                {"id":"mygo","display":"MyGO!!!!!","characters":["anon","tomori"]},
                {"id":"poppin_party","display":"Poppin'Party","characters":["kasumi"]}
            ]}"#,
        )
        .unwrap();
        fs::write(
            dir.path().join("outfit.json"),
            r#"{"characters":{
                "kasumi":{"display":"户山香澄","costumes":{"live_default":"初始","casual":"常服"}},
                "anon":{"display":"千早爱音","costumes":{}}
            }}"#,
        )
        .unwrap();

        let mut index = index_with(
            "kasumi",
            &[
                ("live_default", ModelFormat::Moc),
                ("casual", ModelFormat::Moc),
            ],
        );
        let anon = index_with("anon", &[("casual_spring_01", ModelFormat::Moc3)]);
        index.archives.extend(anon.archives);

        let catalog = Catalog::build(dir.path(), &index);
        assert_eq!(catalog.len(), 2);
        // band.json order puts MyGO first even though "anon" < "kasumi" alphabetically.
        assert_eq!(catalog.characters()[0].key, "anon");
        assert_eq!(catalog.characters()[0].display, "千早爱音");
        assert_eq!(catalog.characters()[1].key, "kasumi");
        assert_eq!(catalog.characters()[1].band_display, "Poppin'Party");
        let costumes = catalog.costumes_of("kasumi");
        assert_eq!(costumes.len(), 2);
        assert!(costumes.iter().any(|costume| costume.display == "初始"));
        assert_eq!(
            catalog.find("anon", "casual_spring_01").unwrap().format,
            ModelFormat::Moc3
        );
        assert_eq!(catalog.search("香澄").len(), 1);
    }

    #[test]
    fn missing_metadata_still_yields_a_catalog() {
        let dir = tempfile::tempdir().unwrap();
        let index = index_with("kasumi", &[("live_default", ModelFormat::Moc)]);
        let catalog = Catalog::build(dir.path(), &index);
        assert_eq!(catalog.len(), 1);
        assert_eq!(catalog.characters()[0].display, "户山香澄");
        assert_eq!(catalog.characters()[0].band_display, "Poppin'Party");
        assert_eq!(catalog.costumes_of("kasumi")[0].display, "队服");
    }

    #[test]
    fn invalid_metadata_uses_the_bundled_names_and_unknown_models_keep_their_keys() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("band.json"), "not json").unwrap();
        fs::write(dir.path().join("outfit.json"), "not json").unwrap();
        let mut index = index_with("kasumi", &[("casual-2023", ModelFormat::Moc)]);
        index
            .archives
            .extend(index_with("custom", &[("outfit", ModelFormat::Moc3)]).archives);
        let catalog = Catalog::build(dir.path(), &index);
        assert_eq!(catalog.character("kasumi").unwrap().display, "户山香澄");
        assert_eq!(
            catalog.find("kasumi", "casual-2023").unwrap().display,
            "常服2023"
        );
        assert_eq!(catalog.character("custom").unwrap().display, "custom");
        assert_eq!(catalog.find("custom", "outfit").unwrap().display, "outfit");
    }
}
