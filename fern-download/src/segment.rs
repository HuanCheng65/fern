//! 一个大文件切成几段并发拉，以及断在半路时怎么接着来。
//!
//! 抄的是 aria2 的两层模型，因为静态均分解决不了真正的问题：把文件按连接数
//! 等分，最慢的那一条就定义了整个文件的完成时间，而「有一条特别慢」在实测里
//! 是常态而不是意外（同一个 55 MB 文件，单流三次跑出 4.6、12.3、23.0 秒）。
//!
//! - **片**（[`PIECE`] 的固定网格）从头到尾不变。所有切点都落在网格上，
//!   所以「下到哪了」能用一张位图表达，而位图能落盘——这是跨进程续传的前提。
//! - **段**是临时分配给某条连接的一段活。它可以被切、被抢：一条连接干完手上
//!   的活，就去剩余量最大的在途段里抢后半截，而不是收工。
//!
//! 位图之外没有别的真相。段死了、进程没了、被抢了一半，重新开工时问的都是
//! 同一个问题：哪些片还没下完。

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};

/// 网格。所有切点都落在它的倍数上——正是 aria2 的 `--piece-length` 在做的事，
/// 它的默认值也是 1 MB。
pub(crate) const PIECE: u64 = 1024 * 1024;

/// 一段至少要有这么多活，才值得为它单开一条连接。
///
/// aria2 的同名参数默认 20 MB，那是为了对公共服务器客气；我们要切的正是
/// 25–60 MB 这一档，按 20 MB 算基本等于不切。
pub(crate) const MIN_SEGMENT: u64 = 2 * PIECE;

/// 一段活再短也不短过这个。
///
/// 每个 Range 请求都是一条新的 HTTP/2 流，而流的初始窗口要几个来回才涨上去。
/// 段切得越碎，这笔爬坡钱付得越多次——实测把一个 55 MB 的文件切成一串顺序
/// 请求，比原来一条长流慢一倍还多。
const MIN_STRIDE: u64 = 4 * PIECE;

/// 一条在途的段。
pub(crate) struct Slot {
    /// 这一段最初从哪开始。不变，只用来判断「哪一片归谁管」。
    from: u64,
    /// 下一个要写的字节。
    at: AtomicU64,
    /// 写到哪为止（不含）。**别人可以把它改小**，把后半截抢走——这就是
    /// 「一条连接卡住不会拖住整个文件」的全部实现。
    end: AtomicU64,
}

impl Slot {
    fn new(from: u64, end: u64) -> Arc<Self> {
        Arc::new(Self {
            from,
            at: AtomicU64::new(from),
            end: AtomicU64::new(end),
        })
    }

    pub(crate) fn at(&self) -> u64 {
        self.at.load(Ordering::Relaxed)
    }

    pub(crate) fn end(&self) -> u64 {
        self.end.load(Ordering::Relaxed)
    }

    pub(crate) fn advance(&self, to: u64) {
        self.at.store(to, Ordering::Relaxed);
    }

    fn remaining(&self) -> u64 {
        self.end().saturating_sub(self.at())
    }
}

/// 一个文件的分段计划：哪些片下完了，哪几段正在途中。
pub(crate) struct Plan {
    size: u64,
    done: Vec<bool>,
    live: Vec<Arc<Slot>>,
    /// 一段活最多领这么长。
    ///
    /// 这个上限省的是带宽：段的范围就是 `Range` 请求的范围，服务器照着发。
    /// 一条工人若一口领走整个文件，之后每被抢一次，它那条连接上就有一批
    /// 「已经在路上、我们却不会再要」的字节——能做的只是断开，断之前发出来
    /// 的都白付了。实测（本机回环，缓冲区最大化）一个 12 MB 的文件能因此多
    /// 收 7 MB。所以开工就按人头分好，抢只用来抹平剩下的不均。
    ///
    /// 但也不能切太碎：每个 Range 请求都是一条新的 HTTP/2 流，而流的初始窗口
    /// 要几个来回才涨上去。切太碎等于把这笔爬坡钱反复付。
    stride: u64,
}

impl Plan {
    pub(crate) fn new(size: u64) -> Self {
        Self {
            size,
            done: vec![false; size.div_ceil(PIECE) as usize],
            live: Vec::new(),
            stride: size.max(PIECE),
        }
    }

    /// 按打算开几条工人，定下一段活的长度。
    pub(crate) fn share_between(&mut self, workers: usize) {
        self.stride = self
            .size
            .div_ceil(workers.max(1) as u64)
            .next_multiple_of(PIECE)
            .max(MIN_STRIDE);
    }

    /// 这些活值得几条工人来干，上限是 `cap`。
    ///
    /// 关键是**不要比段数还多**。多出来的那几条无活可领，只能去抢，而抢一次
    /// 就意味着苦主那条连接上有一批已经在路上的字节要丢掉。抢是用来抹平长尾
    /// 的手段，不该是分活的常态。
    pub(crate) fn workers_wanted(&self, cap: usize) -> usize {
        let remaining = self.size.saturating_sub(self.settled_bytes());
        (remaining.div_ceil(self.stride) as usize).clamp(1, cap)
    }

    /// 已经下完的字节。开工时用来给进度垫底——续传捡回来的那部分也是下过的。
    pub(crate) fn settled_bytes(&self) -> u64 {
        (0..self.done.len())
            .filter(|piece| self.done[*piece])
            .map(|piece| self.piece_span(piece))
            .sum()
    }

    fn piece_span(&self, piece: usize) -> u64 {
        let start = piece as u64 * PIECE;
        (start + PIECE).min(self.size).saturating_sub(start)
    }

    pub(crate) fn is_complete(&self) -> bool {
        self.done.iter().all(|done| *done)
    }

    /// 把 `[from, upto)` 里**整片**写完的那些记上。
    ///
    /// 只记整片：写了半片和没写，对续传是同一件事——重来时那一片整片重下。
    /// 半片的进度不值得记，记了还要多一套「片内偏移」的状态。
    pub(crate) fn mark(&mut self, from: u64, upto: u64) {
        let first = from.div_ceil(PIECE);
        let last = upto / PIECE;
        for piece in first..last {
            if let Some(done) = self.done.get_mut(piece as usize) {
                *done = true;
            }
        }
        // 最后一片通常不满一整片，写到文件尾就算它完成。
        if upto >= self.size && self.size > 0 {
            let last = (self.size - 1) / PIECE;
            if from <= last * PIECE
                && let Some(done) = self.done.get_mut(last as usize)
            {
                *done = true;
            }
        }
    }

    /// 领一段活。先找没人管的空洞，没有就从剩得最多的那一段里抢后半截。
    pub(crate) fn take(&mut self) -> Option<Arc<Slot>> {
        let slot = match self.uncovered() {
            Some((from, end)) => Slot::new(from, end),
            None => self.steal()?,
        };
        self.live.push(slot.clone());
        Some(slot)
    }

    pub(crate) fn retire(&mut self, slot: &Arc<Slot>) {
        self.live.retain(|live| !Arc::ptr_eq(live, slot));
    }

    /// 还没下完、也没人在管的第一段连续区间。
    ///
    /// 续传回来的位图多半是有洞的（上次几条连接各下各的），所以这里必须按
    /// 「连续的空洞」找，而不是从头顺着扫。
    fn uncovered(&self) -> Option<(u64, u64)> {
        let claimed = |piece: usize| {
            let start = piece as u64 * PIECE;
            self.live
                .iter()
                .any(|slot| slot.from <= start && start < slot.end())
        };
        let first = (0..self.done.len()).find(|piece| !self.done[*piece] && !claimed(*piece))?;
        let mut last = first;
        while last + 1 < self.done.len()
            && !self.done[last + 1]
            && !claimed(last + 1)
            // 一段最多领这么长，理由见 `stride`。
            && (last + 1 - first) as u64 * PIECE < self.stride
        {
            last += 1;
        }
        Some((
            first as u64 * PIECE,
            ((last as u64 + 1) * PIECE).min(self.size),
        ))
    }

    /// 从剩余量最大的在途段里抢后半截。
    fn steal(&mut self) -> Option<Arc<Slot>> {
        let victim = self
            .live
            .iter()
            .max_by_key(|slot| slot.remaining())?
            .clone();
        let (at, end) = (victim.at(), victim.end());
        // 抢完两边都还得够一段活，不然只是把开销翻倍。
        if end.saturating_sub(at) < 2 * MIN_SEGMENT {
            return None;
        }
        // 切点必须对齐到网格，否则位图表达不了这一刀。
        let mid = (at + (end - at) / 2).next_multiple_of(PIECE);
        if mid <= at || mid >= end {
            return None;
        }
        // 先把苦主的终点改小，再把后半截接过来。苦主可能已经越过 mid 写了
        // 一小块——那点重叠写的是同样的字节，无害。
        victim.end.store(mid, Ordering::Relaxed);
        Some(Slot::new(mid, end))
    }

    pub(crate) fn state(&self, sha1: Option<&str>) -> ResumeState {
        let mut bits = vec![0u8; self.done.len().div_ceil(8)];
        for (piece, done) in self.done.iter().enumerate() {
            if *done {
                bits[piece / 8] |= 1 << (piece % 8);
            }
        }
        ResumeState {
            size: self.size,
            sha1: sha1.map(str::to_owned),
            piece: PIECE,
            done: bits.iter().map(|byte| format!("{byte:02x}")).collect(),
        }
    }

    /// 上次断在哪。任何一项对不上就返回 `None`——那时候老老实实从头下，
    /// 比拿一份对不上号的位图去拼一个校验永远过不了的文件强。
    pub(crate) fn restore(size: u64, sha1: Option<&str>, state: &ResumeState) -> Option<Self> {
        if state.size != size || state.piece != PIECE {
            return None;
        }
        // 上游换了内容，接着上次下只会拼出一份垃圾。
        if state.sha1.as_deref() != sha1 {
            return None;
        }
        let bits: Vec<u8> = state
            .done
            .as_bytes()
            .chunks(2)
            .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).ok()?, 16).ok())
            .collect::<Option<_>>()?;
        let mut plan = Self::new(size);
        if bits.len() != plan.done.len().div_ceil(8) {
            return None;
        }
        for (piece, done) in plan.done.iter_mut().enumerate() {
            *done = bits[piece / 8] & (1 << (piece % 8)) != 0;
        }
        Some(plan)
    }
}

/// 落在 `<临时文件>.state` 的续传状态。成功之后连同临时文件一起消失。
#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct ResumeState {
    size: u64,
    /// 期望的校验和。它变了说明上游换了内容，这份状态就得作废。
    #[serde(default)]
    sha1: Option<String>,
    piece: u64,
    /// 哪些片下完了。每字节 8 片，低位在前，写成十六进制。
    done: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 开工就按人头分好，没人一口领走整个文件——领走了，后面每被抢一次都要
    /// 白收一批已经在路上的字节。
    #[test]
    fn nobody_claims_the_whole_file_when_help_is_coming() {
        let mut plan = Plan::new(64 * PIECE);
        plan.share_between(8);
        let first = plan.take().expect("第一段");
        assert_eq!((first.at(), first.end()), (0, 8 * PIECE), "只领八分之一");
        let second = plan.take().expect("第二段");
        assert_eq!(
            (second.at(), second.end()),
            (8 * PIECE, 16 * PIECE),
            "第二条往下接着领，不该去抢第一条的"
        );
    }

    /// 但也不能切太碎：每段都是一条新的 h2 流，爬坡钱要付好几次。
    #[test]
    fn segments_never_get_shorter_than_it_is_worth_asking_for() {
        let mut plan = Plan::new(64 * PIECE);
        plan.share_between(64);
        let first = plan.take().expect("第一段");
        assert_eq!(first.end(), MIN_STRIDE, "再多人也不切到下限以下");
    }

    #[test]
    fn a_lone_worker_takes_the_whole_file_in_one_go() {
        // 没人来帮忙时行为必须和分段之前的单流一模一样：一个请求到底。
        let mut plan = Plan::new(64 * PIECE);
        plan.share_between(1);
        let only = plan.take().expect("第一段");
        assert_eq!((only.at(), only.end()), (0, 64 * PIECE));
    }

    #[test]
    fn a_fresh_plan_hands_out_the_whole_file_then_starts_splitting() {
        let mut plan = Plan::new(64 * PIECE);
        let first = plan.take().expect("第一段");
        assert_eq!((first.at(), first.end()), (0, 64 * PIECE));

        // 没有空洞了，第二段只能从第一段身上抢后半截。
        let second = plan.take().expect("第二段");
        assert_eq!(first.end(), 32 * PIECE, "苦主的终点该被改小");
        assert_eq!((second.at(), second.end()), (32 * PIECE, 64 * PIECE));

        // 再要就从剩得最多的那个身上抢——两个一样多时取其一，切点仍在网格上。
        let third = plan.take().expect("第三段");
        assert_eq!(third.at() % PIECE, 0, "切点必须对齐到网格");
        assert!(third.at() > 0 && third.end() <= 64 * PIECE);
    }

    #[test]
    fn stealing_stops_when_there_is_not_enough_work_left_to_be_worth_it() {
        // 一整段只剩不到两段的量时，再切只是把连接开销翻倍。
        let mut plan = Plan::new(3 * PIECE);
        let only = plan.take().expect("第一段");
        assert_eq!(only.end(), 3 * PIECE);
        assert!(plan.take().is_none(), "3 MB 不该切成两段");
    }

    #[test]
    fn only_whole_pieces_count_as_done() {
        let mut plan = Plan::new(10 * PIECE);
        plan.mark(0, 3 * PIECE + 512);
        assert_eq!(plan.settled_bytes(), 3 * PIECE, "写了半片等于没写");
        assert!(!plan.is_complete());

        plan.mark(3 * PIECE, 10 * PIECE);
        assert!(plan.is_complete());
    }

    /// 文件尾那一片通常不满一整片，写到尾就算它完成——否则永远差最后一片。
    #[test]
    fn the_last_short_piece_still_completes() {
        let size = 4 * PIECE + 123;
        let mut plan = Plan::new(size);
        plan.mark(0, size);
        assert!(plan.is_complete());
        assert_eq!(plan.settled_bytes(), size);
    }

    /// 续传要能表达「中间有洞」——上次是几条连接各下各的，位图本来就不连续。
    #[test]
    fn a_restored_plan_hands_out_the_holes_it_left_behind() {
        let mut plan = Plan::new(16 * PIECE);
        plan.mark(0, 4 * PIECE);
        plan.mark(8 * PIECE, 12 * PIECE);
        let state = plan.state(Some("abc"));

        let mut resumed = Plan::restore(16 * PIECE, Some("abc"), &state).expect("接得上");
        assert_eq!(resumed.settled_bytes(), 8 * PIECE);

        // 第一个洞是 [4, 8)，第二个是 [12, 16)。
        let first = resumed.take().expect("第一个洞");
        assert_eq!((first.at(), first.end()), (4 * PIECE, 8 * PIECE));
        let second = resumed.take().expect("第二个洞");
        assert_eq!((second.at(), second.end()), (12 * PIECE, 16 * PIECE));
    }

    /// 上游换了内容、或者大小对不上，上次那份状态一律不认。
    #[test]
    fn a_state_that_describes_another_file_is_refused() {
        let mut plan = Plan::new(16 * PIECE);
        plan.mark(0, 4 * PIECE);
        let state = plan.state(Some("abc"));

        assert!(Plan::restore(16 * PIECE, Some("abc"), &state).is_some());
        assert!(
            Plan::restore(16 * PIECE, Some("def"), &state).is_none(),
            "校验和变了说明上游换了内容"
        );
        assert!(
            Plan::restore(20 * PIECE, Some("abc"), &state).is_none(),
            "大小变了同理"
        );
        assert!(
            Plan::restore(16 * PIECE, None, &state).is_none(),
            "一边有期望值一边没有，也算对不上"
        );
    }

    #[test]
    fn the_state_survives_a_round_trip_through_json() {
        let mut plan = Plan::new(20 * PIECE);
        plan.mark(0, 3 * PIECE);
        plan.mark(9 * PIECE, 11 * PIECE);
        let encoded = serde_json::to_vec(&plan.state(Some("abc"))).expect("encode");
        let decoded: ResumeState = serde_json::from_slice(&encoded).expect("decode");

        let resumed = Plan::restore(20 * PIECE, Some("abc"), &decoded).expect("接得上");
        assert_eq!(resumed.settled_bytes(), 5 * PIECE);
    }

    /// 收工的段要从在途表里摘掉，它那段活才重新变成「没人管」。
    #[test]
    fn work_from_a_retired_segment_goes_back_into_the_pool() {
        let mut plan = Plan::new(8 * PIECE);
        let slot = plan.take().expect("第一段");
        plan.retire(&slot);
        let again = plan.take().expect("重新领");
        assert_eq!((again.at(), again.end()), (0, 8 * PIECE));
    }
}
