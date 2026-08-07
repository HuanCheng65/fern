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

/// 一个不归 Fern 管的游戏目录。
///
/// 大多数人用启动器的方式是把它和 `.minecraft` 放在一起，而不是让它在
/// `AppData` 深处另起一套。那个目录里已经有版本、有存档、有几百个 Mod，
/// 让用户「先导出再导入」等于让他放弃它。
///
/// **我们不搬动它的任何文件。** 记下它在哪、按哪种布局摆放，剩下的照常
/// 工作——缺文件照样补，装模组照样装，只是落点在那边。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExternalGame {
    /// 那个 `.minecraft` 的绝对路径。
    pub root: PathBuf,
    pub isolation: Isolation,
    /// 沿用 Fern 的共享 `assets`/`libraries`，而不是那个目录自带的。
    ///
    /// 默认沿用（`true`）：几个实例共享一份 assets 是几个 G 的差别。改成
    /// `false` 的理由只有一个——那个目录要能被原来的启动器继续单独打开。
    #[serde(default = "yes")]
    pub shared_libraries: bool,
}

fn yes() -> bool {
    true
}

/// 游戏目录摆在哪一层。
///
/// 这是第三方启动器分裂出来的两种约定，而**判断错了的后果是存档看起来消失了**
/// ——游戏会在另一个目录里新建一份空的。所以导入时要真的去看目录里有什么，
/// 不能默认一种。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Isolation {
    /// 存档、mods、配置都在 `.minecraft` 根下，所有版本共用。官方启动器的样子。
    #[default]
    Shared,
    /// 每个版本一套：`.minecraft/versions/<id>/` 下面才是 saves、mods。
    /// HMCL 与 PCL2 的「版本隔离」。
    PerVersion,
}

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
    /// 这一份是为某个外部实例算出来的，游戏目录不再由 id 推导。
    ///
    /// 见 [`DataPaths::scoped`]。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub game_override: Option<PathBuf>,
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
            game_override: None,
        }
    }

    /// 解出某个实例实际用的那套目录。
    ///
    /// 私有实例返回自己；外部实例返回一份**指向那个 `.minecraft` 的** 副本。
    /// 这样下游一个字都不用改——它们要的一直是「versions 在哪、assets 在哪、
    /// 游戏目录在哪」，而不是「这个实例是不是外部的」。判断只发生一次，就在
    /// 每条链路的入口处。
    ///
    /// `version_id` 是版本隔离布局下游戏目录那一层的名字，也就是这个实例真正
    /// 要启动的那份版本描述的 id。
    pub fn scoped(&self, external: Option<&ExternalGame>, version_id: &str) -> Self {
        let Some(external) = external else {
            return self.clone();
        };
        let versions = external.root.join("versions");
        let game = match external.isolation {
            Isolation::Shared => external.root.clone(),
            Isolation::PerVersion => versions.join(version_id),
        };
        Self {
            // 日志、设置、缓存、运行时仍然归 Fern：它们是启动器的东西，不是
            // 那个游戏目录的东西，往别人的目录里塞我们的日志只会添乱。
            root: self.root.clone(),
            instances: self.instances.clone(),
            logs: self.logs.clone(),
            cache: self.cache.clone(),
            runtimes: self.runtimes.clone(),
            versions,
            assets: if external.shared_libraries {
                self.assets.clone()
            } else {
                external.root.join("assets")
            },
            libraries: if external.shared_libraries {
                self.libraries.clone()
            } else {
                external.root.join("libraries")
            },
            game_override: Some(game),
        }
    }

    /// 这台机器上该用哪一套目录。
    ///
    /// 先看便携：可执行文件旁边有 `.minecraft`，或者有一个 `fern-portable`
    /// 标记文件，数据根就跟着可执行文件走。这是「把启动器和游戏放在一起」
    /// 那种用法——U 盘上、游戏盘上、和整合包打包发出去的那一份，用户期待
    /// 它自成一体，而不是在 `AppData` 深处另起一套。
    ///
    /// 标记文件是必需的第二条：有些人的 `.minecraft` 旁边就是桌面，那时候
    /// 只有显式的标记才说明「我要便携」。反过来，`.minecraft` 在旁边这件事
    /// 本身也足够明确——没有人会把启动器随手扔进一个游戏目录旁边。
    pub fn resolve() -> io::Result<Self> {
        match portable_root() {
            Some(root) => Ok(Self::new(root)),
            None => Self::for_current_user(),
        }
    }

    /// 现在用的是不是便携目录。设置页里那一行要说得出来。
    pub fn is_portable(&self) -> bool {
        portable_root().is_some_and(|root| root == self.root)
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
        // 外部实例的游戏目录不由 id 推导——它在别人的目录树里，形状还有两种。
        self.game_override
            .clone()
            .unwrap_or_else(|| self.instance_root(id).join(".minecraft"))
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

/// 便携模式的标记文件。空文件，存在即生效。
pub const PORTABLE_MARKER: &str = "fern-portable";

/// 可执行文件旁边那个 `.minecraft`，前提是它看起来真的是一个游戏目录。
///
/// 首次启动时用来主动问一句「要不要用这个目录里的版本」。把启动器和游戏放在
/// 一起的人，期待的是它自己发现，而不是自己去设置里找一个路径框。
pub fn nearby_game_directory() -> Option<PathBuf> {
    let directory = env::current_exe().ok()?.parent()?.join(".minecraft");
    directory.join("versions").is_dir().then_some(directory)
}

/// 可执行文件旁边那个目录，如果它该被当作数据根的话。
fn portable_root() -> Option<PathBuf> {
    let directory = env::current_exe().ok()?.parent()?.to_path_buf();
    let portable =
        directory.join(PORTABLE_MARKER).exists() || directory.join(".minecraft").is_dir();
    portable.then_some(directory)
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
