//! 账户名册。
//!
//! 之前只有一个账户：设置里一个 `kind`，钥匙串里两个固定键。想同时留着一个
//! 正版号和一个测试用的离线号，只能来回登录——而正版那一次「重新登录」意味着
//! 再走一遍浏览器验证。
//!
//! 拆成两半，判据是**这一条能不能给别人看**：
//!
//! | | 内容 | 在哪 |
//! |---|---|---|
//! | 名册 | id、类型、名字、UUID、皮肤站地址 | `accounts.json` |
//! | 秘密 | 令牌 | 系统钥匙串，一账户一条，键是 `session-<id>` |
//!
//! 这条线和之前一样严：拿到访问令牌就等于拿到这个账号，它永远不进任何一份
//! 用户会打开、会备份、会贴给别人的文件（理由见 credentials.rs）。名册里的
//! 字段全是显示用的——整份泄露也只是别人知道了你叫什么。
//!
//! 离线账户在钥匙串里没有条目：它没有秘密，它的全部内容就是那个名字。

use std::{
    fs,
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};

use crate::{
    DataPaths,
    account::{credentials, microsoft::MicrosoftSession, yggdrasil::YggdrasilSession},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AccountKind {
    #[default]
    Offline,
    Microsoft,
    Authlib,
}

/// 名册里的一条。**这里面没有任何令牌。**
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountRecord {
    /// 发出去就不再改变，钥匙串的键和界面的身份都指着它。名字可以改，id 不行。
    pub id: String,
    pub kind: AccountKind,
    pub player_name: String,
    pub uuid: String,
    /// 外置登录的皮肤站地址。另外两种是 `None`——同一个名字在不同皮肤站是
    /// 不同的人，界面必须说得出是哪一家。
    #[serde(default)]
    pub api_root: Option<String>,
    pub added_at: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Roster {
    /// 当前账户的 id。
    pub active: String,
    pub accounts: Vec<AccountRecord>,
}

impl Roster {
    /// 当前这一个。
    ///
    /// 指针指向一个已经不在的 id 时退回第一条，而不是报错：名册和它的指针
    /// 不同步是能自己修好的小事，不该让人打不开启动器。
    pub fn active(&self) -> Option<&AccountRecord> {
        self.accounts
            .iter()
            .find(|account| account.id == self.active)
            .or_else(|| self.accounts.first())
    }
}

/// 钥匙串里那一条。自带类型，所以名册和钥匙串万一对不上也不会把一份
/// Yggdrasil 令牌当成微软令牌去用。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Secret {
    Microsoft(MicrosoftSession),
    Yggdrasil(YggdrasilSession),
}

fn roster_path(paths: &DataPaths) -> std::path::PathBuf {
    paths.root.join("accounts.json")
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or_default()
}

/// 读名册。
///
/// **只有文件确实不存在时才迁移。** 存在但读不出来（写坏了、被手改坏了）就
/// 交一份空的出去并留痕，不去覆盖它——那份文件里有的是账户，读不懂不等于
/// 可以扔。用户至少还能自己打开看看。
pub fn load(paths: &DataPaths) -> Roster {
    match fs::read(roster_path(paths)) {
        Ok(bytes) => match serde_json::from_slice::<Roster>(&bytes) {
            Ok(roster) => roster,
            Err(error) => {
                let _ = paths.append_log(&format!("[accounts] accounts.json 解析失败：{error}"));
                Roster::default()
            }
        },
        Err(_) => {
            let roster = migrate(paths);
            // 迁移完立刻落盘：下次启动走正常那条路，也不会再去碰遗留的钥匙串条目。
            let _ = save(paths, &roster);
            roster
        }
    }
}

/// 先写 `.part` 再改名。
///
/// 这份文件里是身份。写到一半断电留下半份 JSON，下次启动就成了「一个账户
/// 都没有」——而在线账户的令牌已经跟着 id 存在钥匙串里，名册没了就再也认不
/// 回去。
pub fn save(paths: &DataPaths, roster: &Roster) -> Result<()> {
    fs::create_dir_all(&paths.root).context("创建数据目录")?;
    let bytes = serde_json::to_vec_pretty(roster).context("序列化账户名册")?;
    let path = roster_path(paths);
    let temporary = path.with_extension("part");
    fs::write(&temporary, bytes).context("写入账户名册")?;
    fs::rename(&temporary, &path).context("替换账户名册")
}

/// 从单账户时代搬过来。只在 `accounts.json` 还不存在时跑一次。
///
/// 不做迁移的话，升级一次所有人被登出。
///
/// 钥匙串读不出来（没解锁、没有密钥环）时**什么都不动**：读不到就不删，
/// 遗留的那两条留在原地，至少还有救。能读出来的才搬走并删掉旧键——令牌不该
/// 同时躺在两个地方。
fn migrate(paths: &DataPaths) -> Roster {
    let legacy = crate::data::settings::load(paths).account;
    let mut roster = Roster::default();

    if !legacy.player_name.trim().is_empty()
        && let Ok(record) = offline_record(&legacy.player_name)
    {
        if legacy.kind == AccountKind::Offline {
            roster.active = record.id.clone();
        }
        roster.accounts.push(record);
    }

    match credentials::load_microsoft_session() {
        Ok(Some(session)) => {
            let (name, uuid) = (session.player_name.clone(), session.uuid.clone());
            if let Ok(record) = adopt(
                &name,
                &uuid,
                AccountKind::Microsoft,
                None,
                Secret::Microsoft(session),
            ) {
                if legacy.kind == AccountKind::Microsoft {
                    roster.active = record.id.clone();
                }
                roster.accounts.push(record);
                let _ = credentials::clear_microsoft_session();
            }
        }
        Ok(None) => {}
        Err(error) => {
            let _ = paths.append_log(&format!(
                "[accounts] 迁移微软账户时无法访问系统钥匙串：{error:#}"
            ));
        }
    }

    match credentials::load_session() {
        Ok(Some(session)) => {
            let api_root = Some(session.api_root.clone());
            let (name, uuid) = (session.player_name.clone(), session.uuid.clone());
            if let Ok(record) = adopt(
                &name,
                &uuid,
                AccountKind::Authlib,
                api_root,
                Secret::Yggdrasil(session),
            ) {
                if legacy.kind == AccountKind::Authlib {
                    roster.active = record.id.clone();
                }
                roster.accounts.push(record);
                let _ = credentials::clear_session();
            }
        }
        Ok(None) => {}
        Err(error) => {
            let _ = paths.append_log(&format!(
                "[accounts] 迁移外置账户时无法访问系统钥匙串：{error:#}"
            ));
        }
    }

    roster
}

/// 造一条记录并把它的秘密存进钥匙串。存不进去就不返回记录——一条读不到令牌的
/// 在线账户在界面上会长得和能用的一模一样，点启动才发现登录没了。
fn adopt(
    player_name: &str,
    uuid: &str,
    kind: AccountKind,
    api_root: Option<String>,
    secret: Secret,
) -> Result<AccountRecord> {
    let id = crate::instance::catalog::token()?;
    credentials::store_secret(&id, &secret)?;
    Ok(AccountRecord {
        id,
        kind,
        player_name: player_name.to_owned(),
        uuid: uuid.to_owned(),
        api_root,
        added_at: now(),
    })
}

/// 离线账户。
///
/// UUID 是名字算出来的（和原版服务器的离线算法一致），所以名字就是身份：
/// 改名等于换人，进服之后白名单和存档都会当你是另一个玩家。
fn offline_record(player_name: &str) -> Result<AccountRecord> {
    let credentials = crate::launch::offline_credentials(player_name);
    Ok(AccountRecord {
        id: crate::instance::catalog::token()?,
        kind: AccountKind::Offline,
        player_name: credentials.player_name,
        uuid: credentials.uuid,
        api_root: None,
        added_at: now(),
    })
}

pub fn list(paths: &DataPaths) -> Vec<AccountRecord> {
    load(paths).accounts
}

pub fn active(paths: &DataPaths) -> Option<AccountRecord> {
    load(paths).active().cloned()
}

/// 这个实例该用谁。
///
/// 实例钉住的那一个优先，没钉才跟当前的走。钉住的那一个已经被移除了就当没
/// 钉过——账户没了不该让实例启动不起来，何况屏幕上会显示换成了谁。
///
/// 只有人明确要求时才会有这个字段：启动不写它（见 `launch::prepare`）。
pub fn for_instance(paths: &DataPaths, profile: &crate::InstanceProfile) -> Option<AccountRecord> {
    let roster = load(paths);
    profile
        .account_id
        .as_deref()
        .and_then(|id| roster.accounts.iter().find(|account| account.id == id))
        .or_else(|| roster.active())
        .cloned()
}

pub fn set_active(paths: &DataPaths, id: &str) -> Result<()> {
    let mut roster = load(paths);
    if !roster.accounts.iter().any(|account| account.id == id) {
        return Err(anyhow!("账户不存在"));
    }
    roster.active = id.to_owned();
    save(paths, &roster)
}

/// 加一个离线账户。同名的直接切过去，不新建——两条一模一样的离线账户在界面上
/// 无从分辨，而它们本来就是同一个身份。
pub fn add_offline(paths: &DataPaths, player_name: &str) -> Result<AccountRecord> {
    let record = offline_record(player_name)?;
    let mut roster = load(paths);
    if let Some(existing) = roster
        .accounts
        .iter()
        .find(|account| account.kind == AccountKind::Offline && account.uuid == record.uuid)
        .cloned()
    {
        roster.active = existing.id.clone();
        save(paths, &roster)?;
        return Ok(existing);
    }
    roster.active = record.id.clone();
    roster.accounts.push(record.clone());
    save(paths, &roster)?;
    Ok(record)
}

/// 加一个在线账户，或者刷新已经在册的那一条。
///
/// 同一个 UUID 再登录一次是「重新登录」，不是第二个账户：换的是令牌，人没变。
pub fn adopt_session(paths: &DataPaths, secret: Secret) -> Result<AccountRecord> {
    let (kind, player_name, uuid, api_root) = match &secret {
        Secret::Microsoft(session) => (
            AccountKind::Microsoft,
            session.player_name.clone(),
            session.uuid.clone(),
            None,
        ),
        Secret::Yggdrasil(session) => (
            AccountKind::Authlib,
            session.player_name.clone(),
            session.uuid.clone(),
            Some(session.api_root.clone()),
        ),
    };

    let mut roster = load(paths);
    // 皮肤站不同就是不同的人，UUID 撞了也一样。
    let existing = roster.accounts.iter().position(|account| {
        account.kind == kind && account.uuid == uuid && account.api_root == api_root
    });

    let record = match existing {
        Some(index) => {
            credentials::store_secret(&roster.accounts[index].id, &secret)?;
            // 名字可能在皮肤站那边改过了，以刚拿到的这一份为准。
            roster.accounts[index].player_name = player_name;
            roster.accounts[index].clone()
        }
        None => {
            let record = adopt(&player_name, &uuid, kind, api_root, secret)?;
            roster.accounts.push(record.clone());
            record
        }
    };
    roster.active = record.id.clone();
    save(paths, &roster)?;
    Ok(record)
}

/// 移除一个账户，连同它的令牌。
pub fn remove(paths: &DataPaths, id: &str) -> Result<()> {
    let mut roster = load(paths);
    let Some(index) = roster.accounts.iter().position(|account| account.id == id) else {
        return Ok(());
    };
    let removed = roster.accounts.remove(index);
    // 名册先写：钥匙串删失败（没解锁）时至少界面上它已经走了，而一条没人指向
    // 的令牌是死数据。反过来就糟——令牌删了名册还在，那条账户会一直报错。
    if roster.active == id {
        roster.active = roster
            .accounts
            .first()
            .map(|a| a.id.clone())
            .unwrap_or_default();
    }
    save(paths, &roster)?;
    if removed.kind != AccountKind::Offline {
        credentials::clear_secret(id)?;
    }
    Ok(())
}

/// 给离线账户改名。
///
/// 名字算出 UUID，所以这不是「改个标签」——它换掉了身份。界面必须说清楚。
pub fn rename_offline(paths: &DataPaths, id: &str, player_name: &str) -> Result<AccountRecord> {
    let fresh = crate::launch::offline_credentials(player_name);
    let mut roster = load(paths);
    let account = roster
        .accounts
        .iter_mut()
        .find(|account| account.id == id)
        .ok_or_else(|| anyhow!("账户不存在"))?;
    if account.kind != AccountKind::Offline {
        return Err(anyhow!("仅离线账户的名称由本地指定"));
    }
    account.player_name = fresh.player_name;
    account.uuid = fresh.uuid;
    let updated = account.clone();
    save(paths, &roster)?;
    Ok(updated)
}

/// 这个账户的令牌。离线账户没有，返回 `None` 而不是错误。
pub fn secret(id: &str) -> Result<Option<Secret>> {
    credentials::load_secret(id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    fn paths(name: &str) -> DataPaths {
        let root = env::temp_dir().join(format!("fern-accounts-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        DataPaths::new(root)
    }

    #[test]
    fn offline_accounts_are_their_names() {
        let paths = paths("offline");
        let first = add_offline(&paths, "Steve").expect("add");
        let again = add_offline(&paths, "Steve").expect("add the same name");
        // 同一个名字就是同一个人：不该长出第二条一模一样的记录。
        assert_eq!(first.id, again.id);
        assert_eq!(list(&paths).len(), 1);

        let other = add_offline(&paths, "Alex").expect("add another");
        assert_ne!(other.uuid, first.uuid);
        assert_eq!(list(&paths).len(), 2);
        // 新加的那个成为当前——刚添加完还要再点一下切换是多余的一步。
        assert_eq!(active(&paths).expect("active").id, other.id);

        fs::remove_dir_all(&paths.root).expect("clean up");
    }

    #[test]
    fn renaming_an_offline_account_changes_who_it_is() {
        let paths = paths("rename");
        let account = add_offline(&paths, "Steve").expect("add");
        let renamed = rename_offline(&paths, &account.id, "Alex").expect("rename");
        assert_eq!(renamed.id, account.id);
        // UUID 是名字算出来的，所以改名之后进服就是另一个玩家了。
        assert_ne!(renamed.uuid, account.uuid);

        fs::remove_dir_all(&paths.root).expect("clean up");
    }

    #[test]
    fn removing_the_active_account_leaves_a_valid_pointer() {
        let paths = paths("remove");
        let first = add_offline(&paths, "Steve").expect("add");
        let second = add_offline(&paths, "Alex").expect("add");
        assert_eq!(active(&paths).expect("active").id, second.id);

        remove(&paths, &second.id).expect("remove");
        // 删掉当前那一个之后必须还有一个当前，否则启动按钮不知道用谁。
        assert_eq!(active(&paths).expect("active").id, first.id);

        remove(&paths, &first.id).expect("remove");
        assert!(active(&paths).is_none());

        fs::remove_dir_all(&paths.root).expect("clean up");
    }

    #[test]
    fn an_instance_keeps_the_account_it_last_launched_with() {
        let paths = paths("binding");
        let alt = add_offline(&paths, "Alt").expect("add");
        let main = add_offline(&paths, "Main").expect("add");
        assert_eq!(active(&paths).expect("active").id, main.id);

        let mut profile = crate::InstanceProfile::vanilla(
            crate::InstanceId::parse("modpack").expect("id"),
            "整合包",
            "1.21.1",
        );
        // 没记过就跟当前的走。
        assert_eq!(
            for_instance(&paths, &profile).expect("resolved").id,
            main.id
        );

        // 记过之后，哪怕这期间用大号玩过别的，它还是用小号。
        profile.account_id = Some(alt.id.clone());
        assert_eq!(for_instance(&paths, &profile).expect("resolved").id, alt.id);

        // 记着的那个被移除了，就当没记过——账户没了不该让实例启动不起来。
        remove(&paths, &alt.id).expect("remove");
        assert_eq!(
            for_instance(&paths, &profile).expect("resolved").id,
            main.id
        );

        fs::remove_dir_all(&paths.root).expect("clean up");
    }

    #[test]
    fn a_pointer_at_a_missing_account_falls_back_instead_of_failing() {
        let roster = Roster {
            active: "gone".to_owned(),
            accounts: vec![AccountRecord {
                id: "here".to_owned(),
                kind: AccountKind::Offline,
                player_name: "Steve".to_owned(),
                uuid: "u".to_owned(),
                api_root: None,
                added_at: 0,
            }],
        };
        assert_eq!(roster.active().expect("fallback").id, "here");
        assert!(Roster::default().active().is_none());
    }

    #[test]
    fn a_roster_never_carries_a_token() {
        let roster = Roster {
            active: "a".to_owned(),
            accounts: vec![AccountRecord {
                id: "a".to_owned(),
                kind: AccountKind::Microsoft,
                player_name: "Alex".to_owned(),
                uuid: "u".to_owned(),
                api_root: None,
                added_at: 0,
            }],
        };
        // accounts.json 是一份用户会打开、会备份、可能会截图贴出来的文件。
        // 这条断言存在的意义是：将来往 AccountRecord 上加字段时会撞到它。
        let json = serde_json::to_string(&roster).expect("serialize");
        for forbidden in ["token", "Token", "password", "secret"] {
            assert!(!json.contains(forbidden), "名册里不该出现 {forbidden}");
        }
    }
}
