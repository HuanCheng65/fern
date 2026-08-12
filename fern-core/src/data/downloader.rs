//! 下载器从哪里来。
//!
//! 以前每个调用点自己 `DownloadClient::new(source_order(), 某个数)`——十几处，
//! 每处一个数字，谁也不知道别人那边同时开着几条。真正的问题不在数字选得对不对，
//! 而在于**没有一个地方能回答「现在一共开着多少条」**：补全游戏文件和准备 Java
//! 是并排跑的两条线，各自 64，加起来就是 128。而每处各建一个客户端还意味着
//! 每个阶段重新握一遍 TLS——连接池是跟着客户端走的。
//!
//! 所以全进程一个客户端，连接池、源健康度、验过的文件、全局闸门都挂在它身上；
//! 调用点要的那个数字变成从它分出去的一支（[`DownloadClient::lane`]），仍然管着
//! 「这件事最多同时开几个」，但各支加起来超不过全局那道闸。
//!
//! 客户端是按设置配出来的，所以设置一改就得重配。判据是 `settings::generation()`，
//! 不是逐个字段比对：多配一次的代价只是一个空连接池。

use std::sync::{Arc, OnceLock, RwLock};

use fern_download::{DownloadClient, LogSink, Verified};

use super::settings::{self, network, source_order};
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

/// 每批下完那行账写到 `fern.log` 去。
///
/// 「下载很慢」这句报告，光看进度条分不出慢在对账、传输还是一路重试。这一行
/// 就是为了让下一次「慢」有据可查——它是诊断，写不进去不该影响任何事。
fn log_sink() -> LogSink {
    match DataPaths::for_current_user() {
        Ok(paths) => Arc::new(move |line: &str| {
            let _ = paths.append_log(line);
        }),
        Err(_) => Arc::new(|_: &str| {}),
    }
}

/// 全进程那一个，按当前设置配好的。
///
/// 已经在跑的那些请求还挂在旧的那个上，随它们跑完——正在下载的文件不该因为
/// 用户在设置里换了下载源就断在半路。
fn shared() -> DownloadClient {
    static SHARED: RwLock<Option<(u64, DownloadClient)>> = RwLock::new(None);

    let generation = settings::generation();
    if let Some((configured, client)) = SHARED.read().expect("downloader poisoned").as_ref()
        && *configured == generation
    {
        return client.clone();
    }
    let client = DownloadClient::configured(source_order(), &network())
        .with_verified(ledger())
        .with_log(log_sink());
    *SHARED.write().expect("downloader poisoned") = Some((generation, client.clone()));
    client
}

/// 一支下载器，最多同时开 `concurrency` 个文件。
pub(crate) fn client(concurrency: usize) -> DownloadClient {
    shared().lane(concurrency)
}

/// 不认账本的那一个。用户点「校验」时用，见 [`DownloadClient::rechecking`]。
pub(crate) fn rechecking_client(concurrency: usize) -> DownloadClient {
    client(concurrency).rechecking()
}
