//! 命令行上那些 JVM 参数，在这个 Java 上还认不认（文档 §5 第三条）。
//!
//! 十年前的整合包里全是 `-XX:MaxPermSize`、`-XX:+UseConcMarkSweepGC`。现代
//! JVM 遇到它们不是忽略，是**拒绝启动**——只留一句「Could not create the Java
//! Virtual Machine」，游戏一行日志都没有。这条失败最难查的地方在于它和游戏、
//! 和模组、和这个参数当年为什么被写上去，全都没有关系。
//!
//! ## 按来源分级
//!
//! 去掉一个参数这件事本身没有分歧，**要不要说出来**才有：
//!
//! - 我们自己算出来的、以及版本元数据带的——静悄悄地去掉。用户没写过它，也
//!   就无从解释；说了只会让人以为自己做错了什么。
//! - 实例设置里那一串——**要说**。它是人自己敲进去的（将来也可能是导入整合包
//!   带进来的，见 §5），他有权知道自己写的东西没有生效，以及为什么。
//!
//! 所以这里只提供「判定」和「过滤」，谁该说话由调用方决定：启动那边把去掉的
//! 记进 launch.log，预检查那边把用户自己那串里的每一条说成一句话。
//!
//! ## 表里的数字是怎么来的
//!
//! **实测**（2026-08-11，OpenJDK 8u492 / 21.0.9 / 25.0.3）：表里每一条在
//! Java 8 上都能用（个别带警告），在 21 上一律是 `Unrecognized VM option`，
//! 进程起不来。8 与 21 之间没有别的 JDK 可测，所以**具体是哪一版拿掉的取自
//! 上游的发布说明**——写小了只会让我们多去掉一个本来还能用的参数，写大了则会
//! 放过一个让 JVM 起不来的参数，两害相权，宁可写小。

/// 一条已经消失的参数：前缀，以及它从哪个大版本起不再被接受。
struct Retired {
    /// 匹配用的前缀。`-XX:+Foo` 这样的开关写全，`-XX:Foo=` 这样的带值参数
    /// 写到等号为止。
    prefix: &'static str,
    /// 从这个大版本起，JVM 不再认它。
    removed_in: u16,
}

/// 会让现代 JVM 拒绝启动的那些。
///
/// 只收**真的会导致进程起不来**的：Java 8 上带警告但仍然能跑的（`UseParNewGC`
/// 单独用时那句「Using the ParNew young collector with the Serial old
/// collector」）不算问题，那是 JVM 自己的话，不该由我们代为删掉参数。
const RETIRED: &[Retired] = &[
    // 永久代在 Java 8 就没了（8 上只是警告，9 起直接拒绝）。元空间按需增长，
    // 不需要等价的替代参数——这也是「迁移」而不是「改写」的原因。
    Retired {
        prefix: "-XX:MaxPermSize=",
        removed_in: 9,
    },
    Retired {
        prefix: "-XX:PermSize=",
        removed_in: 9,
    },
    // 分离验证器、快速访问方法：Java 8 忽略，9 起拒绝。
    Retired {
        prefix: "-XX:+UseSplitVerifier",
        removed_in: 9,
    },
    Retired {
        prefix: "-XX:-UseSplitVerifier",
        removed_in: 9,
    },
    Retired {
        prefix: "-XX:+UseFastAccessorMethods",
        removed_in: 9,
    },
    Retired {
        prefix: "-XX:-UseFastAccessorMethods",
        removed_in: 9,
    },
    // 增量式 CMS，两种写法。
    Retired {
        prefix: "-XX:+CMSIncrementalMode",
        removed_in: 9,
    },
    Retired {
        prefix: "-Xincgc",
        removed_in: 9,
    },
    Retired {
        prefix: "-XX:+UseParNewGC",
        removed_in: 10,
    },
    Retired {
        prefix: "-XX:-UseParNewGC",
        removed_in: 10,
    },
    Retired {
        prefix: "-XX:+AggressiveOpts",
        removed_in: 12,
    },
    // CMS 整个被拿掉（JEP 363），连同它那一串调优参数。去掉之后这个实例就
    // 没有人指定 GC 了，我们自己的分配器会接手挑一个——这正是要的结果，所以
    // 过滤必须发生在算内存**之前**。
    Retired {
        prefix: "-XX:+UseConcMarkSweepGC",
        removed_in: 15,
    },
    Retired {
        prefix: "-XX:+UseCMSInitiatingOccupancyOnly",
        removed_in: 15,
    },
    Retired {
        prefix: "-XX:CMSInitiatingOccupancyFraction=",
        removed_in: 15,
    },
    Retired {
        prefix: "-XX:+CMSParallelRemarkEnabled",
        removed_in: 15,
    },
    Retired {
        prefix: "-XX:+CMSClassUnloadingEnabled",
        removed_in: 15,
    },
    Retired {
        prefix: "-XX:+CMSScavengeBeforeRemark",
        removed_in: 15,
    },
    Retired {
        prefix: "-XX:+UseCMSCompactAtFullCollection",
        removed_in: 15,
    },
    Retired {
        prefix: "-XX:CMSFullGCsBeforeCompaction=",
        removed_in: 15,
    },
    // 偏向锁：15 起默认关闭，18 起连参数一起拿掉。
    Retired {
        prefix: "-XX:+UseBiasedLocking",
        removed_in: 18,
    },
    Retired {
        prefix: "-XX:-UseBiasedLocking",
        removed_in: 18,
    },
];

/// 反过来的那一类：太**新**，老 JVM 不认。
///
/// 只有一条，而且是我们自己会给出去的——1.21.5 之后的元数据带着它。
const TOO_NEW: &[Retired] = &[Retired {
    prefix: "--sun-misc-unsafe-memory-access=",
    removed_in: 24,
}];

/// 这个参数在这个 Java 上还能用吗。不能用的话，返回它是从哪一版起消失的。
pub fn retired_in(argument: &str, java_major: u16) -> Option<u16> {
    RETIRED
        .iter()
        .find(|entry| argument.starts_with(entry.prefix))
        .filter(|entry| java_major >= entry.removed_in)
        .map(|entry| entry.removed_in)
}

/// 这个参数在这个 Java 上是不是**还没有**。
fn premature(argument: &str, java_major: u16) -> bool {
    TOO_NEW
        .iter()
        .any(|entry| argument.starts_with(entry.prefix) && java_major < entry.removed_in)
}

/// 去掉这个 Java 不认的那些，返回留下的和去掉的。
///
/// 去掉的那一份要由调用方处置：进日志，或者（用户自己写的那些）说成一句话。
pub fn prune(arguments: Vec<String>, java_major: u16) -> (Vec<String>, Vec<String>) {
    arguments.into_iter().partition(|argument| {
        retired_in(argument, java_major).is_none() && !premature(argument, java_major)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 一份 2014 年整合包的启动参数，原样。
    const ANCIENT: &[&str] = &[
        "-XX:MaxPermSize=256m",
        "-XX:PermSize=128m",
        "-XX:+UseConcMarkSweepGC",
        "-XX:+CMSIncrementalMode",
        "-XX:+UseParNewGC",
        "-XX:+AggressiveOpts",
        "-XX:+UseFastAccessorMethods",
        "-XX:+CMSClassUnloadingEnabled",
        "-XX:CMSInitiatingOccupancyFraction=75",
        "-Xmn512m",
        "-Dfml.ignorePatchDiscrepancies=true",
    ];

    #[test]
    fn a_ten_year_old_argument_list_survives_on_java_8_and_is_migrated_on_21() {
        let ancient: Vec<String> = ANCIENT.iter().map(|item| (*item).to_owned()).collect();

        // Java 8 上它们都还能用，一个都不该动——那台机器上这份参数是对的。
        let (kept, dropped) = prune(ancient.clone(), 8);
        assert_eq!(kept.len(), ancient.len(), "去掉了 {dropped:?}");
        assert!(dropped.is_empty());

        // Java 21 上，凡是会让 JVM 拒绝启动的都要走。
        let (kept, dropped) = prune(ancient, 21);
        assert_eq!(dropped.len(), 9);
        // 剩下的必须是那些今天仍然合法的。
        assert_eq!(
            kept,
            vec!["-Xmn512m", "-Dfml.ignorePatchDiscrepancies=true"]
        );
    }

    /// 边界要按各自那一版算，不是一刀切。
    #[test]
    fn each_option_disappears_at_its_own_version() {
        assert_eq!(retired_in("-XX:MaxPermSize=256m", 8), None);
        assert_eq!(retired_in("-XX:MaxPermSize=256m", 9), Some(9));
        // CMS 活到 15 才被拿掉，Java 11 上它还是好的。
        assert_eq!(retired_in("-XX:+UseConcMarkSweepGC", 11), None);
        assert_eq!(retired_in("-XX:+UseConcMarkSweepGC", 15), Some(15));
        assert_eq!(retired_in("-XX:+UseBiasedLocking", 17), None);
        assert_eq!(retired_in("-XX:+UseBiasedLocking", 18), Some(18));
        // 没听说过的一律不动：这张表是白名单式的删除，不是黑名单式的放行。
        assert_eq!(retired_in("-XX:+UseZGC", 25), None);
        assert_eq!(retired_in("-Dsomething=1", 25), None);
        assert_eq!(retired_in("-Xmx4G", 25), None);
    }

    /// 反过来那一条：太新的参数在老 JVM 上同样起不来。
    #[test]
    fn an_option_that_does_not_exist_yet_is_dropped_too() {
        let arguments = vec!["--sun-misc-unsafe-memory-access=allow".to_owned()];
        assert!(prune(arguments.clone(), 21).0.is_empty());
        assert_eq!(prune(arguments, 24).0.len(), 1);
    }
}
