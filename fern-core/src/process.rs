//! 起子进程时的平台细节。
//!
//! 现在只有一件事，但它出现在三个地方，所以它在这里而不是散在各处。

use std::process::Command;

/// Windows 上不给子进程开控制台窗口。
///
/// 启动器自己是 GUI 子系统的程序（`windows_subsystem = "windows"`），运行时没有
/// 控制台；而 `java.exe` 是控制台子系统的。**父进程没有控制台时，Windows 会给
/// 这样的子进程新建一个**，重定向 stdout/stderr 并不改变这件事——于是玩家先看到
/// 一个什么都不显示的黑框，它一直挂到游戏退出，关掉它就等于杀掉游戏。
///
/// 调试构建里看不到这个现象：那时启动器自己有控制台，子进程直接附着上去。
///
/// 不改用 `javaw.exe`：一个标志管得住所有子进程（探测 Java、Forge 的 processor
/// 也在起 java），而换 javaw 只解决启动游戏这一处，还要多判断一层「这份运行时
/// 有没有 javaw」。
pub(crate) fn without_console(command: &mut Command) {
    with_creation_flags(command, 0);
}

/// 同上，外加别的创建标志。
///
/// **`creation_flags` 是覆盖不是按位或。** 分两次调用，后一次会把前一次设的东西
/// 悄悄抹掉——所以要同时设的标志只能在这里合成一个值。
pub(crate) fn with_creation_flags(command: &mut Command, extra: u32) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // std 里没有这个常量，值来自 Win32 的 processthreadsapi.h。
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW | extra);
    }

    #[cfg(not(windows))]
    {
        let _ = (command, extra);
    }
}
