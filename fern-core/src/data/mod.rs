//! 数据层：东西在磁盘上的哪里、用户的偏好是什么、拉下来的元数据还新不新。
//!
//! 三个文件是一条线上的三个问题——`mod.rs` 说**位置**（`DataPaths` 是全仓库
//! 唯一一份目录布局），`settings.rs` 说**偏好**，`metacache.rs` 说**时效**。
//!
//! 别的层都依赖这一层。反过来只有一处：`settings` 里那份旧的账户字段还引着
//! 名册的类型，那是迁移用的遗留结构，迁完就没有了。

pub(crate) mod metacache;
pub(crate) mod settings;

use std::{
    env, fs, io,
    path::{Path, PathBuf},
};

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
    /// 版本描述也放在 Fern 这边，不放进那个游戏目录。
    ///
    /// 官方启动器那一系的目录里本来就有 `versions/`，接手它就该用那一份
    /// （默认 `false`）。Prism 那一系不是：它的实例目录下只有游戏文件，版本
    /// 描述在启动器自己的全局缓存里。往别人的 `.minecraft` 里凭空造一个
    /// `versions/` 既不是它的约定，也违背这个模块「不动别人的文件」的底线。
    #[serde(default)]
    pub shared_versions: bool,
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

/// 游戏文件那一层的名字。见 [`DataPaths`]。
pub const GAME_DIRECTORY: &str = ".minecraft";

/// Stable on-disk layout shared by every launcher subsystem.
///
/// 数据根下面分成两半，**游戏的东西和启动器的东西不混在一起**：
///
/// ```text
/// <root>/.minecraft/  assets  libraries  versions   游戏要读的
/// <root>/            instances  runtimes  logs  cache  settings.json   启动器自己的
/// ```
///
/// 分界是「游戏运行时会不会去读它」：资源、依赖库、版本描述会，所以它们按
/// 官方的名字摆在一个标准的 `.minecraft` 里——那个目录因此也能被别的启动器
/// 直接打开。Java 运行时是**运行**游戏的东西而不是游戏读的东西，日志和缓存
/// 更是我们自己的账本，它们都留在外面。
///
/// 每个实例自己的游戏目录仍然是 `instances/<id>/.minecraft`：那是它的存档和
/// 模组，和上面这份共享的资源不是一回事。
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
        let game = root.join(GAME_DIRECTORY);
        Self {
            assets: game.join("assets"),
            libraries: game.join("libraries"),
            versions: game.join("versions"),
            runtimes: root.join("runtimes"),
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
            versions: if external.shared_versions {
                self.versions.clone()
            } else {
                versions
            },
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
    /// 便携模式要**显式**开启：可执行文件旁边放一个 `fern-portable` 标记文件。
    /// 开启之后数据放在 `fern-data/` 这一个子目录里，整个文件夹拷走即可迁移。
    ///
    /// 曾经的规则是「旁边有 `.minecraft` 就算便携」，而且直接把可执行文件所在
    /// 的目录当数据根。两条都是错的：把启动器和 `.minecraft` 放在一起是最常见
    /// 的摆法，它表达的是「这里有个现成的游戏目录」（见 [`nearby_game_directory`]），
    /// 不是「把你的数据摊在我旁边」；而摊开的后果是用户的文件夹里凭空多出
    /// assets、libraries、instances、versions、logs、cache 和 settings.json。
    pub fn resolve() -> io::Result<Self> {
        match portable_root() {
            Some(root) => {
                gather_scattered(&root)?;
                Ok(Self::new(root))
            }
            None => {
                let default = platform_data_root().ok_or_else(|| {
                    io::Error::new(
                        io::ErrorKind::NotFound,
                        "unable to resolve user data directory",
                    )
                })?;
                Ok(Self::new(redirected(default)))
            }
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
        self.tidy_game_directories()?;
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

    /// 把早先摊在数据根下的 assets、libraries、versions 挪进 `.minecraft/`。
    ///
    /// 同一个卷上的目录改名，不复制内容——几个 GB 的资源不会被搬一遍。目标
    /// 已经存在时两边都不动：那说明有人手工搬过一半，合并两棵目录树不是这里
    /// 该替他做的决定，而少下载的文件补全时会自己长回来。
    fn tidy_game_directories(&self) -> io::Result<()> {
        let game = self.root.join(GAME_DIRECTORY);
        for (name, current) in [
            ("assets", &self.assets),
            ("libraries", &self.libraries),
            ("versions", &self.versions),
        ] {
            let legacy = self.root.join(name);
            // 只认「就在数据根下、而且我们现在要用的不是它」这一种情况。
            if legacy == *current || !legacy.is_dir() || current.exists() {
                continue;
            }
            fs::create_dir_all(&game)?;
            fs::rename(&legacy, current)?;
        }
        Ok(())
    }

    /// 共享游戏文件那一层：`<root>/.minecraft`。
    ///
    /// 说的是 Fern 自己那一份，与外部实例无关——[`scoped`](Self::scoped) 之后
    /// `assets` 之类会指向别人的目录，而 `root` 始终是我们的。
    pub fn shared_game_root(&self) -> PathBuf {
        self.root.join(GAME_DIRECTORY)
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

/// 便携模式下数据放在哪一个子目录里。
pub const PORTABLE_DIRECTORY: &str = "fern-data";

/// 便携模式下的数据根：可执行文件旁边的 `fern-data/`，前提是有标记文件。
///
/// **只认标记，不猜。** 猜的代价是把数据写进一个不属于我们的文件夹：旁边有
/// `.minecraft` 说明不了什么，而 MultiMC 那一系的便携目录同样有 `instances`、
/// `libraries`、`assets`，光看目录名分不出是谁的。
/// 数据根被迁走之后，默认位置只剩这一张字条，里面写着新家的绝对路径。
///
/// 是 `.txt` 是有意的：用户点开默认目录看到唯一一个文件时，双击它就能知道
/// 自己的数据去了哪。写与撤都在 `storage::migrate` 里。
pub(crate) const REDIRECT_FILE: &str = "data-root.txt";

/// 平台默认的数据根，不考虑字条。迁移要知道字条该写在哪。
pub(crate) fn default_data_root() -> Option<PathBuf> {
    platform_data_root()
}

/// 默认位置留了字条就跟着字条走。
///
/// 不追第二层：字条只会由迁移写在默认位置，指向的地方就是数据本身。字条
/// 指着不存在的地方也照样跟——那多半是移动硬盘没插；退回默认位置会凭空
/// 造出一套全新的空数据，比一个「目录打不开」的报错更让人心慌。
fn redirected(default: PathBuf) -> PathBuf {
    let Ok(text) = fs::read_to_string(default.join(REDIRECT_FILE)) else {
        return default;
    };
    let target = PathBuf::from(text.trim());
    if target.is_absolute() && target != default {
        target
    } else {
        default
    }
}

fn portable_root() -> Option<PathBuf> {
    let directory = env::current_exe().ok()?.parent()?.to_path_buf();
    directory
        .join(PORTABLE_MARKER)
        .exists()
        .then(|| directory.join(PORTABLE_DIRECTORY))
}

/// 把早先摊在可执行文件同级的那一套收进 `fern-data/`。
///
/// 便携模式曾经直接把数据根定在可执行文件所在的目录上，于是那个文件夹里多出
/// 七个目录和一个 settings.json。这里只搬**我们自己建的那几个名字**，
/// `.minecraft` 一个字都不碰——它多半是用户自己的游戏目录。
///
/// 签名要求 `settings.json` 和 `instances/` 同时在：单看 `instances/` 会把
/// MultiMC 的便携目录也算进来，而那不是我们该动的东西。
fn gather_scattered(root: &Path) -> io::Result<()> {
    let Some(outside) = root.parent() else {
        return Ok(());
    };
    if root.exists()
        || !outside.join("settings.json").is_file()
        || !outside.join("instances").is_dir()
    {
        return Ok(());
    }
    fs::create_dir_all(root)?;
    for name in [
        "assets",
        "libraries",
        "versions",
        "instances",
        "runtimes",
        "logs",
        "cache",
        "settings.json",
    ] {
        let from = outside.join(name);
        let to = root.join(name);
        if from.exists() && !to.exists() {
            fs::rename(&from, &to)?;
        }
    }
    Ok(())
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
        // 游戏要读的东西在 `.minecraft/` 里，启动器自己的在外面。
        assert_eq!(
            paths.assets,
            PathBuf::from("/tmp/fern-contract-test/.minecraft/assets")
        );
        assert_eq!(
            paths.versions,
            PathBuf::from("/tmp/fern-contract-test/.minecraft/versions")
        );
        assert_eq!(
            paths.runtimes,
            PathBuf::from("/tmp/fern-contract-test/runtimes")
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

    /// 布局分开之前的数据要跟过来，而不是留在原地变成孤儿。
    #[test]
    fn an_older_layout_is_moved_under_the_game_directory() {
        let root = env::temp_dir().join(format!("fern-tidy-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        for name in ["assets/indexes", "libraries/net", "versions/1.21.1"] {
            fs::create_dir_all(root.join(name)).expect("create legacy layout");
        }
        fs::write(root.join("versions/1.21.1/1.21.1.json"), b"{}").expect("write version");

        let paths = DataPaths::new(&root);
        paths.ensure_exists().expect("create data layout");

        assert!(paths.versions.join("1.21.1/1.21.1.json").is_file());
        assert!(paths.assets.join("indexes").is_dir());
        assert!(paths.libraries.join("net").is_dir());
        // 挪走之后数据根下不该还剩一个空壳。
        assert!(!root.join("versions").exists());

        // 再跑一次不会把已经就位的东西再动一遍。
        paths.ensure_exists().expect("second run");
        assert!(paths.versions.join("1.21.1/1.21.1.json").is_file());

        fs::remove_dir_all(root).expect("remove test tree");
    }

    /// 便携模式曾经把数据摊在可执行文件同级，改规则不能把那些实例丢在原地。
    #[test]
    fn a_scattered_portable_layout_is_gathered_into_one_directory() {
        let outside = env::temp_dir().join(format!("fern-gather-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&outside);
        for name in ["instances/oak", "versions/1.21.1", "logs"] {
            fs::create_dir_all(outside.join(name)).expect("create scattered layout");
        }
        fs::write(outside.join("settings.json"), b"{}").expect("write settings");
        // 用户自己的游戏目录也在这里。它不归我们动。
        fs::create_dir_all(outside.join(".minecraft/saves/world")).expect("create game directory");

        let root = outside.join(PORTABLE_DIRECTORY);
        gather_scattered(&root).expect("gather");
        assert!(root.join("instances/oak").is_dir());
        assert!(root.join("settings.json").is_file());
        assert!(!outside.join("instances").exists());
        assert!(outside.join(".minecraft/saves/world").is_dir());

        // 收拢过一次就不再动了，哪怕之后旁边又出现了同名的文件夹。
        fs::create_dir_all(outside.join("instances/from-somewhere-else")).expect("create");
        gather_scattered(&root).expect("second run");
        assert!(outside.join("instances/from-somewhere-else").is_dir());

        fs::remove_dir_all(outside).expect("remove test tree");
    }

    /// 别的启动器的便携目录长得很像：MultiMC 那一系同样有 instances、
    /// libraries、assets。搬错了是在动不属于我们的数据。
    #[test]
    fn another_launchers_directory_is_left_alone() {
        let outside = env::temp_dir().join(format!("fern-foreign-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&outside);
        for name in ["instances/1.21.1", "libraries", "assets"] {
            fs::create_dir_all(outside.join(name)).expect("create foreign layout");
        }
        fs::write(outside.join("multimc.cfg"), b"").expect("write config");

        gather_scattered(&outside.join(PORTABLE_DIRECTORY)).expect("gather");
        assert!(outside.join("instances/1.21.1").is_dir());
        assert!(!outside.join(PORTABLE_DIRECTORY).exists());

        fs::remove_dir_all(outside).expect("remove test tree");
    }
}
