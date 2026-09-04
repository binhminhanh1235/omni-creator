use std::{
    fmt, fs,
    path::{Component, Path, PathBuf},
    str::FromStr,
};

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::{Error, Result};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum LogicalUri {
    Workspace(String),
    Project(String),
    Library(String),
    Artifact(String),
}

impl LogicalUri {
    pub fn parse(value: &str) -> Result<Self> {
        value.parse()
    }

    pub fn as_str(&self) -> String {
        match self {
            Self::Workspace(path) => format!("workspace://{path}"),
            Self::Project(path) => format!("project://{path}"),
            Self::Library(path) => format!("library://{path}"),
            Self::Artifact(id) => format!("artifact://{id}"),
        }
    }
}

impl fmt::Display for LogicalUri {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.as_str())
    }
}

impl FromStr for LogicalUri {
    type Err = Error;

    fn from_str(value: &str) -> Result<Self> {
        let (scheme, rest) = value
            .split_once("://")
            .ok_or_else(|| Error::InvalidLogicalUri(value.to_owned()))?;
        if rest.is_empty() {
            return Err(Error::InvalidLogicalUri(value.to_owned()));
        }
        match scheme {
            "workspace" => Ok(Self::Workspace(validate_logical_path(rest)?)),
            "project" => Ok(Self::Project(validate_logical_path(rest)?)),
            "library" => Ok(Self::Library(validate_logical_path(rest)?)),
            "artifact" if !rest.contains('/') && !rest.contains('\\') => {
                Ok(Self::Artifact(rest.to_owned()))
            }
            _ => Err(Error::InvalidLogicalUri(value.to_owned())),
        }
    }
}

impl Serialize for LogicalUri {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.as_str())
    }
}

impl<'de> Deserialize<'de> for LogicalUri {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::parse(&value).map_err(serde::de::Error::custom)
    }
}

fn validate_logical_path(value: &str) -> Result<String> {
    if value.starts_with('/') || value.contains('\\') || value.contains('\0') {
        return Err(Error::InvalidLogicalUri(value.to_owned()));
    }
    for component in Path::new(value).components() {
        match component {
            Component::Normal(_) => {}
            _ => return Err(Error::PathEscape(value.to_owned())),
        }
    }
    Ok(value.to_owned())
}

#[derive(Debug, Clone)]
pub struct PathResolver {
    data_root: PathBuf,
}

impl PathResolver {
    pub fn new(data_root: impl AsRef<Path>) -> Result<Self> {
        let data_root = fs::canonicalize(data_root)?;
        Ok(Self { data_root })
    }

    pub fn data_root(&self) -> &Path {
        &self.data_root
    }

    pub fn resolve(&self, uri: &LogicalUri, project_id: Option<&str>) -> Result<PathBuf> {
        let (allowed_root, relative) = match uri {
            LogicalUri::Workspace(path) => (self.data_root.clone(), path.as_str()),
            LogicalUri::Library(path) => (self.data_root.join("library"), path.as_str()),
            LogicalUri::Project(path) => {
                let project_id = project_id.ok_or_else(|| {
                    Error::InvalidLogicalUri("project:// URI requires project_id".to_owned())
                })?;
                validate_project_id(project_id)?;
                (
                    self.data_root.join("projects").join(project_id),
                    path.as_str(),
                )
            }
            LogicalUri::Artifact(id) => {
                return Err(Error::ArtifactResolutionRequired(id.clone()));
            }
        };

        let candidate = allowed_root.join(relative);
        ensure_no_symlink_escape(&allowed_root, &candidate)?;
        Ok(candidate)
    }
}

fn validate_project_id(project_id: &str) -> Result<()> {
    if project_id.is_empty()
        || project_id.contains('/')
        || project_id.contains('\\')
        || project_id == "."
        || project_id == ".."
    {
        return Err(Error::PathEscape(project_id.to_owned()));
    }
    Ok(())
}

fn ensure_no_symlink_escape(allowed_root: &Path, candidate: &Path) -> Result<()> {
    let mut current = allowed_root.to_path_buf();
    if current.exists() && fs::symlink_metadata(&current)?.file_type().is_symlink() {
        return Err(Error::PathEscape(current.display().to_string()));
    }

    let relative = candidate
        .strip_prefix(allowed_root)
        .map_err(|_| Error::PathEscape(candidate.display().to_string()))?;
    for component in relative.components() {
        if let Component::Normal(segment) = component {
            current.push(segment);
            if current.exists() && fs::symlink_metadata(&current)?.file_type().is_symlink() {
                return Err(Error::PathEscape(current.display().to_string()));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn logical_uri_round_trip() {
        let uri = LogicalUri::parse("workspace://projects/P001/audio/S01.wav").unwrap();
        assert_eq!(uri.as_str(), "workspace://projects/P001/audio/S01.wav");
    }

    #[test]
    fn rejects_traversal() {
        assert!(LogicalUri::parse("workspace://../secret").is_err());
        assert!(LogicalUri::parse("project:///etc/passwd").is_err());
        assert!(LogicalUri::parse("library://a\\..\\secret").is_err());
    }
}
