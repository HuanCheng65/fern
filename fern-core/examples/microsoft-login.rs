//! 走一遍微软登录，把结果原样打出来。
//!
//! 这是给白名单申请用的一次性工具（文档 §3.1 前置手续第 4 条）：微软要求
//! 应用先有实际调用记录才受理审批，所以在申请之前必须真的登录失败一次，
//! 让 `login_with_xbox` 返回 403。
//!
//! ```sh
//! cargo run -p fern-core --example microsoft-login
//! ```
//!
//! 白名单批下来之后，同一条链会一路走到底并打印出角色名——那时候这个例子
//! 就变成一个「验证正版登录还通不通」的自检工具，不必删掉。
//!
//! 界面上的登录走的是同一套函数（设置 → 账户 → 微软账户），这里只是把它
//! 从命令行也暴露出来，因为在装齐 WebView 依赖之前 UI 跑不起来。

use std::io::Write;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let challenge = fern_core::begin_microsoft_login().await?;

    println!();
    println!("  打开   {}", challenge.verification_uri);
    println!("  输入   {}", challenge.user_code);
    println!();
    println!(
        "（{} 秒内有效，等待你在浏览器里完成……）",
        challenge.expires_in
    );
    std::io::stdout().flush()?;

    match fern_core::finish_microsoft_login(&challenge, &fern_core::Nudge::new()).await {
        Ok(session) => {
            println!();
            println!("登录成功：{} / {}", session.player_name, session.uuid);
            println!("白名单已经生效，正版登录可用。");
            // 记进名册，界面那边就能直接看到已登录。
            match fern_core::DataPaths::for_current_user()
                .map_err(anyhow::Error::from)
                .and_then(|paths| {
                    fern_core::adopt_account(&paths, fern_core::Secret::Microsoft(session))
                }) {
                Ok(record) => println!("已加入账户名册（{}）。", record.id),
                Err(error) => println!("没能记进名册：{error:#}"),
            }
        }
        Err(error) => {
            println!();
            println!("停在：{error:#}");
            println!();
            println!("如果上面说的是白名单没批，那这一次失败正是申请的前提——");
            println!("现在可以去 https://aka.ms/mce-reviewappid 提交审批了。");
        }
    }
    Ok(())
}
