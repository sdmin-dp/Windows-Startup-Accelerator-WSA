# WSA — Windows Startup Accelerator

一个用 **Rust** 编写的 Windows 开机启动优化器，带 **TUI** 设置面板，主打**高效加速开机**。

![Rust](https://img.shields.io/badge/Rust-1.70%2B-orange.svg)
![License](https://img.shields.io/badge/license-MIT-blue.svg)
![Dependencies](https://img.shields.io/badge/dependencies-0-brightgreen.svg)

> **注意**：WSA 是**开机优化器**，不是启动项管理器。它优化的是 Windows 开机过程本身（服务启动策略、系统设置、资源调度），而不是逐个决定哪些程序开机运行。

## 优化原理

Windows 开机慢的主要瓶颈：非必要服务在登录瞬间争抢磁盘 I/O、视觉特效合成开销、大量自启动程序同时涌入。WSA 从四个层面解决：

1. **服务延迟启动** —— 把 SysMain、Windows Search、打印后台、BITS、Windows Update 等非关键服务改为「延迟自动启动」（微软官方支持，登录后再就绪，不禁用功能）
2. **系统设置 tweak** —— 关闭窗口动画、加快菜单显示、最佳性能视觉效果等
3. **启动程序错峰** —— 高影响自启动程序分批延迟启动，避免资源踩踏
4. **开机耗时测量** —— 可选功能，记录每次登录耗时并展示优化前后对比（默认关闭）

## 特点

- **零第三方依赖** —— 直接调用 Win32 API，不依赖 crates.io。二进制约 400 KB，启动 < 1 ms
- **纯标准库 TUI** —— 手写 ANSI 转义与控制台输入
- **完全可逆** —— 所有服务/注册表改动都记录原值到 `%LOCALAPPDATA%\wsa\backups.tsv`，按 `R` 一键恢复
- **安全护栏** —— 每项标注风险等级与预计收益，用户逐项勾选，绝不擅自改动
- **完全离线可构建**

## 内置优化项（21 项）

| 类别 | 示例 |
| --- | --- |
| 服务延迟（13 项） | Windows Search、SysMain、打印后台、BITS、Windows Update、传递优化、诊断策略、错误报告、IP Helper、推送通知、连接设备平台、传真、Xbox Live |
| 系统设置（5 项） | 最佳性能视觉效果、关闭窗口动画、缩短服务终止超时、自动结束无响应任务、加快菜单显示 |
| 启动程序（1 项） | 启动程序错峰延迟 |
| 耗时测量（1 项） | 开机耗时测量（默认关闭） |
| 其他（1 项） | 预读优化 (Prefetch) |

## 安装与构建

```powershell
git clone https://github.com/sdmin-dp/Windows-Startup-Accelerator-WSA.git
cd Windows-Startup-Accelerator-WSA
cargo build --release
.\target\release\wsa.exe
```

默认使用 `x86_64-pc-windows-msvc` 目标（需 Visual Studio C++ 生成工具）。若只有 MinGW-w64，可用 `cargo +stable-x86_64-pc-windows-gnu build --release`。

## TUI 使用

运行 `wsa` 打开设置面板，三个页面：

- **1 优化项** —— 浏览所有优化项，显示类别/风险/收益/当前状态；`空格` 勾选，`A` 一键应用，`R` 恢复
- **2 开机耗时** —— 展示历史登录耗时曲线（需先开启测量并重启）
- **3 帮助** —— 快捷键与说明

### 快捷键

| 按键 | 功能 |
| --- | --- |
| `↑` `↓` / `PgUp` `PgDn` / `Home` `End` | 移动选择 |
| **鼠标左键点击** | 选中该行 |
| **鼠标滚轮** | 上下滚动列表 |
| `空格` / `Enter` | 勾选/取消该优化项 |
| `A` | 一键应用所有已勾选项 |
| `R` | 恢复所有更改（按两次确认） |
| `S` | 重新检测当前状态 |
| `/` 或 `F` | 筛选优化项 |
| `1` `2` `3` | 切换页面 |
| `Esc` | 退出 |

## 命令行

```
wsa                  启动 TUI 优化设置面板
wsa --scan           列出所有可优化项及当前状态（只读，安全）
wsa --apply          应用所有已启用的优化项
wsa --daemon         开机后台执行（供登录自启调用）
wsa --autostart-on   注册登录时自动运行 WSA
wsa --autostart-off  移除登录时自动运行
wsa --help           显示帮助
```

## 开机自动执行

```powershell
# 1) 以管理员身份打开 TUI，勾选需要的优化项并按 A 应用一次
# 2) 注册登录自启
wsa --autostart-on
```

这会在任务计划程序注册 `WSA Startup Accelerator` 用户级任务，每次登录时运行 `wsa --daemon`：执行错峰启动并记录开机耗时，全程无窗口。

## 权限说明

- **HKCU 注册表 tweak 与错峰启动** —— 当前用户权限即可
- **服务延迟启动与 HKLM 注册表** —— 需要以管理员身份运行（TUI 中对这类项标注「需管理员」）
- 检测功能（`--scan`、TUI 列表）只读，任何权限都可运行

## 安全与恢复

- 所有改动都是**可逆**的：服务记录原启动类型 + 延迟标志，注册表记录原值
- `R` 键按两次确认后恢复所有更改，并清空备份
- 风险等级：安全（官方支持、无感）/ 低（轻微行为变化）/ 中（影响特定功能，描述中有说明）
- 不会禁用任何服务，只改为延迟启动；不会删除任何数据

## 许可证

MIT © sdmin-dp
