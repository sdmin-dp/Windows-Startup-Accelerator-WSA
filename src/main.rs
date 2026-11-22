mod console;
mod daemon;
mod engine;
mod frame;
mod model;
mod optmodel;
mod optimize;
mod registry;
mod scanner;
mod tui;
mod winapi;

use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(|s| s.as_str()) {
        Some("--daemon") => {
            daemon::run_daemon();
            ExitCode::SUCCESS
        }
        Some("--autostart-on") => match daemon::set_autostart(true) {
            Ok(()) => { println!("已注册开机自启任务"); ExitCode::SUCCESS }
            Err(e) => { eprintln!("注册失败: {}", e); ExitCode::FAILURE }
        },
        Some("--autostart-off") => match daemon::set_autostart(false) {
            Ok(()) => { println!("已移除开机自启任务"); ExitCode::SUCCESS }
            Err(e) => { eprintln!("移除失败: {}", e); ExitCode::FAILURE }
        },
        Some("--apply") => {
            let mut catalog = optimize::catalog();
            optimize::detect_all(&mut catalog);
            let r = optimize::apply_enabled(&catalog);
            let _ = daemon::append_backups(&r.backups);
            println!("已应用 {} 项优化，失败 {} 项（其中 {} 项因权限不足，请以管理员身份运行），预计节省 {:.1} 秒",
                r.applied, r.failed, r.needs_admin, r.gain);
            ExitCode::SUCCESS
        }
        Some("--scan") => {
            let mut catalog = optimize::catalog();
            optimize::detect_all(&mut catalog);
            println!("{:<28} {:<10} {:<6} {:<8} {}", "优化项", "类别", "风险", "预计收益", "当前状态");
            for o in catalog {
                println!("{:<28} {:<10} {:<6} {:>5.1}s    {}",
                    o.name, o.category.label(), o.risk.label(), o.gain_sec, o.state.label());
            }
            ExitCode::SUCCESS
        }
        Some("--keytest") => {
            match console::ConsoleGuard::new() {
                Some(guard) => guard.run_key_diagnostics(),
                None => eprintln!("错误：当前环境没有可用的控制台，无法进行按键诊断。"),
            }
            ExitCode::SUCCESS
        }
        Some("--input-selftest") => {
            // Verifies the decode pipeline without needing a console, so it
            // produces a report under any launch method (service, pipe, IDE).
            let mut lines: Vec<String> = vec!["WSA 输入解码自检".to_string()];
            let report = std::path::PathBuf::from(r"C:\Users\Public\wsa-input-selftest.txt");

            let ok_layout = std::mem::size_of::<console::INPUT_RECORD>() == 20
                && std::mem::align_of::<console::INPUT_RECORD>() == 4
                && std::mem::size_of::<console::KEY_EVENT_RECORD>() == 16;
            lines.push(format!(
                "结构体布局: INPUT_RECORD={}字节/对齐{}, KEY_EVENT_RECORD={}字节 -> {}",
                std::mem::size_of::<console::INPUT_RECORD>(),
                std::mem::align_of::<console::INPUT_RECORD>(),
                std::mem::size_of::<console::KEY_EVENT_RECORD>(),
                if ok_layout { "正确" } else { "错误" }
            ));

            let decoded = console::selftest_decode();
            let ok_decode = decoded.is_ok();
            match &decoded {
                Ok(samples) => {
                    for s in samples {
                        lines.push(format!("  解码: {}", s));
                    }
                }
                Err(e) => lines.push(format!("  解码失败: {}", e)),
            }

            let verdict = ok_layout && ok_decode;
            lines.push(format!("结果: {}", if verdict { "通过" } else { "失败" }));
            let _ = std::fs::write(&report, lines.join("\n"));
            for l in &lines { println!("{}", l); }
            if verdict { ExitCode::SUCCESS } else { ExitCode::FAILURE }
        }
        Some("--help") | Some("-h") => { print_help(); ExitCode::SUCCESS }
        None => { tui::run(); ExitCode::SUCCESS }
        Some(other) => {
            eprintln!("未知参数: {}", other);
            print_help();
            ExitCode::FAILURE
        }
    }
}

fn print_help() {
    println!("WSA - Windows Startup Accelerator (开机启动优化器)");
    println!();
    println!("用法:");
    println!("  wsa                  启动 TUI 优化设置面板");
    println!("  wsa --scan           列出所有可优化项及当前状态");
    println!("  wsa --keytest        按键诊断（排查输入问题）");
    println!("  wsa --input-selftest 输入解码自检（验证键盘链路）");
    println!("  wsa --apply          应用所有已启用的优化项");
    println!("  wsa --daemon         开机后台执行（供登录自启调用）");
    println!("  wsa --autostart-on   注册登录时自动运行 WSA");
    println!("  wsa --autostart-off  移除登录时自动运行");
    println!("  wsa --help           显示帮助");
}
