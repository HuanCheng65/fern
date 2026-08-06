use std::{env, fs, io, path::PathBuf};

use serde::{Deserialize, Serialize};

/// Stable on-disk layout shared by every launcher subsystem.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DataPaths {
    pub root: PathBuf,
    pub assets: PathBuf,
    pub libraries: PathBuf,
    pub runtimes: PathBuf,
    pub versions: PathBuf,
    pub instances: PathBuf,
}

impl DataPaths {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        let root = root.into();
        Self {
            assets: root.join("assets"),
            libraries: root.join("libraries"),
            runtimes: root.join("runtimes"),
            versions: root.join("versions"),
            instances: root.join("instances"),
            root,
        }
    }

    /// Resolve the conventional per-user data directory for the host platform.
    pub fn for_current_user() -> io::Result<Self> {
        let root = platform_data_root().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                "unable to resolve user data directory",
            )
        })?;
        Ok(Self::new(root))
    }

    pub fn ensure_exists(&self) -> io::Result<()> {
        for path in [
            &self.root,
            &self.assets,
            &self.libraries,
            &self.runtimes,
            &self.versions,
            &self.instances,
        ] {
            fs::create_dir_all(path)?;
        }
        Ok(())
    }

    pub fn instance_root(&self, id: &str) -> PathBuf {
        self.instances.join(id)
    }

    pub fn instance_config(&self, id: &str) -> PathBuf {
        self.instance_root(id).join("instance.json")
    }

    pub fn game_directory(&self, id: &str) -> PathBuf {
        self.instance_root(id).join(".minecraft")
    }
}

fn platform_data_root() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        env::var_os("APPDATA")
            .map(PathBuf::from)
            .map(|path| path.join("Fern"))
    }

    #[cfg(target_os = "macos")]
    {
        env::var_os("HOME")
            .map(PathBuf::from)
            .map(|path| path.join("Library/Application Support/Fern"))
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|| {
                env::var_os("HOME")
                    .map(PathBuf::from)
                    .map(|path| path.join(".local/share"))
            })
            .map(|path| path.join("fern"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_separates_shared_and_instance_files() {
        let paths = DataPaths::new("/tmp/fern-contract-test");
        assert_eq!(
            paths.assets,
            PathBuf::from("/tmp/fern-contract-test/assets")
        );
        assert_eq!(
            paths.game_directory("cinder-valley"),
            PathBuf::from("/tmp/fern-contract-test/instances/cinder-valley/.minecraft")
        );
        assert_eq!(
            paths.instance_config("cinder-valley"),
            PathBuf::from("/tmp/fern-contract-test/instances/cinder-valley/instance.json")
        );
    }

    #[test]
    fn ensure_exists_creates_all_shared_directories() {
        let root = env::temp_dir().join(format!("fern-paths-test-{}", std::process::id()));
        let paths = DataPaths::new(&root);
        paths.ensure_exists().expect("create data layout");

        for path in [
            paths.assets,
            paths.libraries,
            paths.runtimes,
            paths.versions,
            paths.instances,
        ] {
            assert!(path.is_dir(), "{} should be a directory", path.display());
        }

        fs::remove_dir_all(root).expect("remove test data layout");
    }
}
