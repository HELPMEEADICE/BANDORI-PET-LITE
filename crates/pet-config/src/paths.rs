//! Filesystem layout shared by both processes (spec §4.3).

use std::path::{Path, PathBuf};

/// Environment variable that overrides the application root (used by tests and
/// by running straight out of `target/debug` during development).
pub const ROOT_ENV: &str = "BANDORI_PET_ROOT";

/// Directory layout of one installation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Paths {
    root: PathBuf,
    cache: PathBuf,
}

impl Paths {
    /// Discovers the layout: `$BANDORI_PET_ROOT`, else the executable directory.
    pub fn discover() -> Self {
        let root = std::env::var_os(ROOT_ENV)
            .map(PathBuf::from)
            .or_else(|| {
                std::env::current_exe()
                    .ok()
                    .and_then(|exe| exe.parent().map(Path::to_path_buf))
            })
            .unwrap_or_else(|| PathBuf::from("."));
        Self::with_root(root)
    }

    /// Builds the layout on top of an explicit root directory.
    pub fn with_root(root: impl Into<PathBuf>) -> Self {
        let root = root.into();
        let cache = local_app_data()
            .unwrap_or_else(|| root.clone())
            .join("BandoriPetLite")
            .join("cache");
        Self { root, cache }
    }

    /// Installation root (contains both executables).
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// `config.json` — single writer is `bandori-pet`.
    pub fn config(&self) -> PathBuf {
        self.root.join("config.json")
    }

    /// `models/` holding the `.zst` archives and the model index.
    pub fn models(&self) -> PathBuf {
        self.root.join("models")
    }

    /// `models/.bandori_model_index.json`.
    pub fn model_index(&self) -> PathBuf {
        self.models().join(".bandori_model_index.json")
    }

    /// `assets/` holding `band.json`, `outfit.json`, icons and fonts.
    pub fn assets(&self) -> PathBuf {
        self.root.join("assets")
    }

    /// `logs/`.
    pub fn logs(&self) -> PathBuf {
        self.root.join("logs")
    }

    /// `ui/`.
    pub fn ui(&self) -> PathBuf {
        self.root.join("ui")
    }

    /// `%LOCALAPPDATA%\BandoriPetLite\cache` — extracted model costumes.
    pub fn cache(&self) -> &Path {
        &self.cache
    }

    /// `%LOCALAPPDATA%\BandoriPetLite` — cache parent.
    pub fn local_data(&self) -> Option<PathBuf> {
        self.cache.parent().map(Path::to_path_buf)
    }

    /// Path of the settings executable that lives next to `bandori-pet.exe`.
    pub fn ui_executable(&self) -> PathBuf {
        self.root.join(exe_name("bandori-ui"))
    }

    /// Path of the pet executable.
    pub fn pet_executable(&self) -> PathBuf {
        self.root.join(exe_name("bandori-pet"))
    }
}

fn exe_name(stem: &str) -> String {
    if cfg!(windows) {
        format!("{stem}.exe")
    } else {
        stem.to_owned()
    }
}

fn local_app_data() -> Option<PathBuf> {
    std::env::var_os("LOCALAPPDATA").map(PathBuf::from)
}

/// Rows shown on the About page (`label`, `value`).
pub fn about_lines(paths: &Paths, config: &crate::Config) -> Vec<(String, String)> {
    vec![
        ("版本".to_owned(), env!("CARGO_PKG_VERSION").to_owned()),
        ("模型目录".to_owned(), paths.models().display().to_string()),
        ("缓存目录".to_owned(), paths.cache().display().to_string()),
        ("日志目录".to_owned(), paths.logs().display().to_string()),
        (
            "当前角色".to_owned(),
            format!("{} / {}", config.character, config.costume),
        ),
        ("配置文件".to_owned(), paths.config().display().to_string()),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_matches_spec() {
        let paths = Paths::with_root("C:/app");
        assert_eq!(paths.config(), PathBuf::from("C:/app/config.json"));
        assert_eq!(paths.models(), PathBuf::from("C:/app/models"));
        assert!(paths.model_index().ends_with(".bandori_model_index.json"));
        assert!(paths.cache().ends_with("BandoriPetLite/cache"));
    }
}

