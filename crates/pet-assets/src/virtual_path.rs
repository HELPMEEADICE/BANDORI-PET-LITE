//! The `models/<archive>.zst::<costume>/model.json` virtual path syntax.

use std::path::{Component, Path, PathBuf};

/// Separator between the archive path and the member path.
pub const SEP: &str = "::";

/// A model reference split into archive and member.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelPath {
    /// Archive part, exactly as written in `config.json` (`models/kasumi.zst`).
    pub archive: String,
    /// Member path inside the archive, normalized to forward slashes.
    pub member: String,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum VirtualPathError {
    /// No `::` separator.
    #[error("path has no '::' separator: {0}")]
    MissingSeparator(String),
    /// Empty archive part.
    #[error("path has an empty archive part: {0}")]
    EmptyArchive(String),
    /// Empty or unsafe member part.
    #[error("unsafe member path: {0}")]
    UnsafeMember(String),
}

impl ModelPath {
    /// Parses `archive::member`.
    pub fn parse(spec: &str) -> Result<Self, VirtualPathError> {
        let (archive, member) = spec
            .split_once(SEP)
            .ok_or_else(|| VirtualPathError::MissingSeparator(spec.to_owned()))?;
        let archive = archive.trim();
        if archive.is_empty() {
            return Err(VirtualPathError::EmptyArchive(spec.to_owned()));
        }
        let member = normalize_member(member)?;
        Ok(Self {
            archive: archive.to_owned(),
            member,
        })
    }

    /// Builds a model path from its parts.
    pub fn new(archive: impl Into<String>, member: impl AsRef<str>) -> Result<Self, VirtualPathError> {
        Ok(Self {
            archive: archive.into(),
            member: normalize_member(member.as_ref())?,
        })
    }

    /// Rebuilds the canonical `archive::member` string.
    pub fn to_spec(&self) -> String {
        format!("{}{}{}", self.archive, SEP, self.member)
    }

    /// Costume directory: the first member segment.
    pub fn costume(&self) -> Option<&str> {
        self.member.split('/').next().filter(|part| !part.is_empty())
    }

    /// Resolves the archive against the installation root.
    pub fn archive_path(&self, root: &Path) -> PathBuf {
        let normalized = self.archive.replace('\\', "/");
        let candidate = PathBuf::from(&normalized);
        if candidate.is_absolute() {
            candidate
        } else {
            root.join(candidate)
        }
    }

    /// `true` when the member is a Cubism 3/4/5 `*.model3.json`.
    pub fn is_moc3(&self) -> bool {
        self.member.to_ascii_lowercase().ends_with(".model3.json")
    }
}

/// Normalizes an archive member path, rejecting absolute paths and `..`.
pub fn normalize_member(member: &str) -> Result<String, VirtualPathError> {
    let unified = member.replace('\\', "/");
    let mut parts = Vec::new();
    for component in unified.split('/') {
        match component {
            "" | "." => continue,
            ".." => return Err(VirtualPathError::UnsafeMember(member.to_owned())),
            part => parts.push(part),
        }
    }
    if parts.is_empty() {
        return Err(VirtualPathError::UnsafeMember(member.to_owned()));
    }
    let normalized = parts.join("/");
    // Double-check with the platform parser: Windows also treats `C:` and UNC
    // prefixes as absolute, which `split('/')` alone would not catch.
    let path = Path::new(&normalized);
    if path.is_absolute() || path.components().any(|c| matches!(c, Component::Prefix(_))) {
        return Err(VirtualPathError::UnsafeMember(member.to_owned()));
    }
    Ok(normalized)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_bandori_reference() {
        let path = ModelPath::parse("models/kasumi.zst::live_event_295_ur/model.json").unwrap();
        assert_eq!(path.archive, "models/kasumi.zst");
        assert_eq!(path.member, "live_event_295_ur/model.json");
        assert_eq!(path.costume(), Some("live_event_295_ur"));
        assert!(!path.is_moc3());
        assert_eq!(
            path.to_spec(),
            "models/kasumi.zst::live_event_295_ur/model.json"
        );
    }

    #[test]
    fn detects_moc3_members() {
        let path = ModelPath::parse(
            "models/anon.zst::casual_spring_01/adv_live2d_anon_002_casual_spring_01.model3.json",
        )
        .unwrap();
        assert!(path.is_moc3());
    }

    #[test]
    fn rejects_traversal_and_absolute_members() {
        for spec in [
            "models/kasumi.zst::../evil/model.json",
            "models/kasumi.zst::C:/windows/system32/x.json",
            "models/kasumi.zst::",
        ] {
            assert!(ModelPath::parse(spec).is_err(), "{spec} should be rejected");
        }
    }

    #[test]
    fn rejects_missing_separator() {
        assert_eq!(
            ModelPath::parse("models/kasumi.zst"),
            Err(VirtualPathError::MissingSeparator(
                "models/kasumi.zst".to_owned()
            ))
        );
    }

    #[test]
    fn normalizes_windows_separators() {
        let path =
            ModelPath::parse("models\\kasumi.zst::live_default\\data\\model.moc").unwrap();
        assert_eq!(path.member, "live_default/data/model.moc");
    }
}
