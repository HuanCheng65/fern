//! 账户（文档 §3）。
//!
//! 三种账户类型统一到一个地方，回答文档列出的那三个问题：
//!
//! ```text
//! ensure_fresh()        静默刷新，必要时把要用的东西也准备好
//! launch_credentials()  name / uuid / access_token / user_type
//! extra_jvm_args()      authlib-injector 用
//! ```
//!
//! 文档给的是 trait，这里写成 enum。理由是这个集合是封闭的——三种账户全部由
//! Fern 自己实现，没有第三方需要接进来——而 `ensure_fresh` 必须是 async，
//! async trait 在今天还不是 dyn 兼容的，用 trait 就得为「返回三者之一」额外
//! 绕一圈。enum 把同一个抽象点保留了下来，代价更小。
//!
//! 之前这三种是在 `launch.rs` 里一个 match 里就地展开的，登录相关的逻辑
//! 混在拼命令行的中间。搬到这里之后，`launch` 只需要问「凭据是什么、要不要
//! 额外的 JVM 参数」。
//!
//! 这一层的其余部分：名册在 `roster.rs`（账户是复数，见文档 §3.4），两种要
//! 联网的登录各占一个文件（`microsoft.rs`、`yggdrasil.rs`，离线那种不需要谁
//! 来实现），令牌的保管处是 `credentials.rs`——只有它碰系统钥匙串。皮肤在
//! `skin.rs`，它走的是公开档案，和这里的登录态没有关系。

pub(crate) mod credentials;
pub(crate) mod microsoft;
pub(crate) mod roster;
pub(crate) mod skin;
pub(crate) mod yggdrasil;

use std::path::PathBuf;

use anyhow::{Context, Result, anyhow};
use fern_download::DownloadEvent;
use tokio::sync::mpsc::UnboundedSender;

use crate::{Credentials, DataPaths, launch::offline_credentials};

use roster::{AccountKind, AccountRecord, Secret};

/// 这次启动用谁的身份。
///
/// 每一种在线账户都带着自己的账户 id：刷新出来的新令牌要存回**它自己**那一条，
/// 而现在名册里可能同时有三个微软账户。
#[derive(Debug, Clone)]
pub enum Account {
    /// 离线模式。UUID 由名字推出来，和原版服务器的离线算法一致。
    Offline { player_name: String },
    /// 微软正版。
    Microsoft {
        id: String,
        session: microsoft::MicrosoftSession,
    },
    /// 外置登录。`injector` 在 `ensure_fresh` 之后才有值。
    Yggdrasil {
        id: String,
        session: yggdrasil::YggdrasilSession,
        injector: Option<PathBuf>,
        prefetched: String,
    },
}

impl Account {
    /// 名册里当前那一个。
    pub fn active(paths: &DataPaths) -> Result<Self> {
        let record =
            roster::active(paths).ok_or_else(|| anyhow!("尚未添加账户，请在设置中添加"))?;
        Self::load(&record)
    }

    /// 名册里指定的那一条。
    ///
    /// 记录说的是「这个账户是谁」，钥匙串说的是「凭什么是他」。两边对不上——
    /// 有记录没令牌——是一种真实且必须说清楚的状态：钥匙串没解锁、或者用户在
    /// 系统的密码管理器里手动删过。
    pub fn load(record: &AccountRecord) -> Result<Self> {
        match record.kind {
            AccountKind::Offline => Ok(Self::Offline {
                player_name: record.player_name.clone(),
            }),
            kind => {
                let missing = || {
                    anyhow!(
                        "{} 的登录信息已不在系统钥匙串中，请重新登录",
                        record.player_name
                    )
                };
                match (kind, roster::secret(&record.id)?.ok_or_else(missing)?) {
                    (AccountKind::Microsoft, Secret::Microsoft(session)) => Ok(Self::Microsoft {
                        id: record.id.clone(),
                        session,
                    }),
                    (AccountKind::Authlib, Secret::Yggdrasil(session)) => Ok(Self::Yggdrasil {
                        id: record.id.clone(),
                        session,
                        injector: None,
                        prefetched: String::new(),
                    }),
                    _ => Err(missing()),
                }
            }
        }
    }

    /// 静默刷新，顺带把启动要用的东西准备好。
    ///
    /// 启动前刷一次，比让玩家进服时才被踢好——那时候的报错和登录没有任何
    /// 表面关联，用户会去查网络、查服务器、查模组。
    pub async fn ensure_fresh(
        &mut self,
        paths: &DataPaths,
        events: &UnboundedSender<DownloadEvent>,
    ) -> Result<()> {
        match self {
            Self::Offline { .. } => Ok(()),
            Self::Microsoft { id, session } => {
                let fresh = microsoft::ensure_fresh(session)
                    .await
                    .context("微软令牌刷新失败，请重新登录")?;
                if &fresh != session {
                    crate::store_secret(id, &Secret::Microsoft(fresh.clone()))?;
                    *session = fresh;
                }
                Ok(())
            }
            Self::Yggdrasil {
                id,
                session,
                injector,
                prefetched,
            } => {
                let fresh = yggdrasil::ensure_fresh(session)
                    .await
                    .context("外置登录令牌刷新失败，请重新登录")?;
                if &fresh != session {
                    crate::store_secret(id, &Secret::Yggdrasil(fresh.clone()))?;
                    *session = fresh;
                }
                *injector = Some(yggdrasil::ensure_injector(paths, events).await?);
                // 预取失败不该拦住启动：injector 自己会去请求一次，只是慢一点。
                *prefetched = yggdrasil::prefetched(&session.api_root)
                    .await
                    .unwrap_or_default();
                Ok(())
            }
        }
    }

    /// 进启动参数的那四个值。
    pub fn launch_credentials(&self) -> Result<Credentials> {
        match self {
            Self::Offline { player_name } => {
                // 这条规则是 Minecraft 自己的，只对离线模式成立——另外两种的
                // 名字由服务端给，我们不该拦。
                if !(3..=16).contains(&player_name.len())
                    || !player_name
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
                {
                    return Err(anyhow!("离线模式名称需为 3-16 位字母、数字或下划线"));
                }
                Ok(offline_credentials(player_name))
            }
            Self::Microsoft { session, .. } => Ok(Credentials {
                player_name: session.player_name.clone(),
                uuid: session.uuid.clone(),
                access_token: session.access_token.clone(),
                user_type: "msa".to_owned(),
            }),
            Self::Yggdrasil { session, .. } => Ok(Credentials {
                player_name: session.player_name.clone(),
                uuid: session.uuid.clone(),
                access_token: session.access_token.clone(),
                // authlib-injector 接管之后，游戏那边看到的仍然是一个正常的
                // 在线会话，所以是 msa 而不是 legacy。
                user_type: "msa".to_owned(),
            }),
        }
    }

    /// 额外的 JVM 参数。只有外置登录有。
    ///
    /// 调用方要把它们插在最前面：javaagent 得在游戏的任何一个类被加载之前
    /// 挂上去。
    pub fn extra_jvm_args(&self) -> Vec<String> {
        match self {
            Self::Yggdrasil {
                session,
                injector: Some(injector),
                prefetched,
                ..
            } => yggdrasil::jvm_arguments(injector, &session.api_root, prefetched),
            // 没跑过 ensure_fresh 就没有 injector，那时候一个参数都不该给：
            // 给了一半（有 prefetched 没有 agent）比一个都不给更难查。
            _ => Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn yggdrasil() -> yggdrasil::YggdrasilSession {
        yggdrasil::YggdrasilSession {
            api_root: "https://littleskin.cn/api/yggdrasil".to_owned(),
            access_token: "token".to_owned(),
            client_token: "client".to_owned(),
            uuid: "abc".to_owned(),
            player_name: "Steve".to_owned(),
        }
    }

    #[test]
    fn only_offline_names_are_validated_locally() {
        // 离线的 UUID 是从名字算的，名字不合法就没有稳定的身份。
        let bad = Account::Offline {
            player_name: "a".to_owned(),
        };
        assert!(bad.launch_credentials().is_err());
        let spaced = Account::Offline {
            player_name: "has space".to_owned(),
        };
        assert!(spaced.launch_credentials().is_err());

        let good = Account::Offline {
            player_name: "FernPlayer".to_owned(),
        };
        let credentials = good.launch_credentials().expect("valid name");
        assert_eq!(credentials.user_type, "legacy");

        // 皮肤站的角色名可以是中文、可以超过 16 位，我们不该拦。
        let remote = Account::Yggdrasil {
            id: "a".to_owned(),
            session: yggdrasil::YggdrasilSession {
                player_name: "一个很长的中文角色名字".to_owned(),
                ..yggdrasil()
            },
            injector: None,
            prefetched: String::new(),
        };
        assert!(remote.launch_credentials().is_ok());
    }

    #[test]
    fn an_unprepared_yggdrasil_account_contributes_no_arguments() {
        // 还没 ensure_fresh 过：给一半参数（有 prefetched 没有 agent）比一个
        // 都不给更难查——游戏会正常启动，但皮肤站根本没接上。
        let account = Account::Yggdrasil {
            id: "a".to_owned(),
            session: yggdrasil(),
            injector: None,
            prefetched: "eyJ9".to_owned(),
        };
        assert!(account.extra_jvm_args().is_empty());

        let ready = Account::Yggdrasil {
            id: "a".to_owned(),
            session: yggdrasil(),
            injector: Some(PathBuf::from("/fern/authlib-injector.jar")),
            prefetched: "eyJ9".to_owned(),
        };
        let arguments = ready.extra_jvm_args();
        assert_eq!(arguments.len(), 2);
        assert!(arguments[0].starts_with("-javaagent:"));
    }

    #[test]
    fn the_other_two_kinds_never_add_jvm_arguments() {
        assert!(
            Account::Offline {
                player_name: "Steve".to_owned()
            }
            .extra_jvm_args()
            .is_empty()
        );
        assert!(
            Account::Microsoft {
                id: "a".to_owned(),
                session: microsoft::MicrosoftSession {
                    refresh_token: "r".to_owned(),
                    access_token: "a".to_owned(),
                    uuid: "u".to_owned(),
                    player_name: "Alex".to_owned(),
                    expires_at: 0,
                }
            }
            .extra_jvm_args()
            .is_empty()
        );
    }

    #[test]
    fn online_accounts_report_themselves_as_msa() {
        // 老版本的参数模板按 user_type 分支；离线必须是 legacy，联机的两种
        // 都是 msa——authlib-injector 接管之后游戏看到的也是正常在线会话。
        let msa = Account::Microsoft {
            id: "a".to_owned(),
            session: microsoft::MicrosoftSession {
                refresh_token: "r".to_owned(),
                access_token: "a".to_owned(),
                uuid: "u".to_owned(),
                player_name: "Alex".to_owned(),
                expires_at: 0,
            },
        };
        assert_eq!(msa.launch_credentials().unwrap().user_type, "msa");

        let ygg = Account::Yggdrasil {
            id: "a".to_owned(),
            session: yggdrasil(),
            injector: None,
            prefetched: String::new(),
        };
        assert_eq!(ygg.launch_credentials().unwrap().user_type, "msa");
    }
}
