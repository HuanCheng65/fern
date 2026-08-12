//! 下载器从哪里来。
//!
//! 以前每个调用点自己 `DownloadClient::new(source_order(), 某个数)`——十几处，
//! 每处一个数字，谁也不知道别人那边同时开着几条。真正的问题不在数字选得对不对，
//! 而在于**没有一个地方能回答「现在一共开着多少条」**：补全游戏文件和准备 Java
//! 是并排跑的两条线，各自 64，加起来就是 128。
//!
//! 所以入口收到这里。眼下这个函数仍然每次新建一个客户端，只把「验过的文件」
//! 那本账做成全进程共用一本——两条线各记各的会互相覆盖，而它正是省下每次启动
//! 重算几百兆哈希的那本账。剩下的（共用连接池、全局并发上限、限速）接着往这里加。

use std::sync::{Arc, OnceLock};

use fern_download::{DownloadClient, Verified};

use super::settings::source_order;
use crate::DataPaths;

/// 验过的文件，全进程一本。
///
/// 找不到数据目录就退回一本关着的空账：那时候连缓存目录都无从谈起，而下载
/// 本身不该因此失败，只是回到每次都重算的老样子。
fn ledger() -> Arc<Verified> {
    static LEDGER: OnceLock<Arc<Verified>> = OnceLock::new();
    LEDGER
        .get_or_init(|| match DataPaths::for_current_user() {
            Ok(paths) => Verified::at(paths.cache.join("verified.json")),
            Err(_) => Arc::new(Verified::default()),
        })
        .clone()
}

/// 一个下载器，最多同时开 `concurrency` 个文件。
pub(crate) fn client(concurrency: usize) -> DownloadClient {
    DownloadClient::new(source_order(), concurrency).with_verified(ledger())
}

/// 不认账本的那一个。用户点「校验」时用，见 [`DownloadClient::rechecking`]。
pub(crate) fn rechecking_client(concurrency: usize) -> DownloadClient {
    client(concurrency).rechecking()
}
