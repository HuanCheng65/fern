// Windows 上不要那个控制台窗口。
//
// 少了这一行，双击 exe 会先弹出一个黑色控制台，主界面才在它后面出现——而那个
// 控制台关掉就等于杀掉进程。子系统是链接期的属性，运行时无法补救，所以它只能
// 写在这里，而且必须在 crate 的最前面。
//
// 只在 release 关掉：开发时那个窗口正是 `println!` 和 panic 信息的去处。
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    fern_ui_lib::run();
}
