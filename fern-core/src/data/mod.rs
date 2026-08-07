//! 数据层：东西在磁盘上的哪里、用户的偏好是什么、拉下来的元数据还新不新。
//!
//! 三个文件是一条线上的三个问题——`mod.rs` 说**位置**（`DataPaths` 是全仓库
//! 唯一一份目录布局），`settings.rs` 说**偏好**，`metacache.rs` 说**时效**。
//!
//! 别的层都依赖这一层。反过来只有一处：`settings` 里那份旧的账户字段还引着
//! 名册的类型，那是迁移用的遗留结构，迁完就没有了。

pub(crate) mod metacache;
pub(crate) mod settings;

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
    pub logs: PathBuf,
    /// 会过期的元数据（版本清单、加载器版本列表）。
    ///
    /// 和 `versions`、`assets` 分开：那两个虽然也是下载来的，但它们是**成品**
    /// ——游戏要读的东西，删了就等于卸载。这里放的全是随时可以整个删掉、下次
    /// 联网自己长回来的东西，所以「清理缓存」能安全地只清它。
    pub cache: PathBuf,
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
            logs: root.join("logs"),
            cache: root.join("cache"),
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
            &self.logs,
            &self.cache,
        ] {
            fs::create_dir_all(path)?;
        }
        Ok(())
    }

    /// 用户设置。放在数据根目录下，和实例、日志平级——它是一份能被打开、
    /// 备份、贴给别人的文件，不是藏起来的缓存。
    pub fn settings_path(&self) -> PathBuf {
        self.root.join("settings.json")
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

    pub fn instance_log_directory(&self, id: &str) -> PathBuf {
        self.logs.join("instances").join(id)
    }

    pub fn fern_log_path(&self) -> PathBuf {
        self.logs.join("fern.log")
    }

    pub fn append_log(&self, message: &str) -> io::Result<()> {
        use std::io::Write;
        fs::create_dir_all(&self.logs)?;
        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.fern_log_path())?;
        writeln!(file, "{message}")
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
