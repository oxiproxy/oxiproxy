# `status` 命令 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 为 controller / node / client 三个组件各新增一个 `status` 子命令，查询守护进程运行状态（PID 探活 + 运行时长 + 内存）并在 Linux 上额外展示 systemd 服务状态。

**Architecture:** 跨平台进程探测逻辑下沉到 `common/src/process_status.rs`（零新增依赖，Linux 读 `/proc`，Windows 仅探活）；systemd 查询函数加进已有的 `common/src/systemd.rs`（该文件 `#![cfg(target_os = "linux")]`）。三个组件 `main.rs` 各加一个 `Status` CLI 变体 + 一个 `print_status()` 薄封装，调用 common 并打印、设置退出码。

**Tech Stack:** Rust 2021，clap（CLI），libc（已是 workspace 依赖，用于 `kill(pid,0)` 探活和 `sysconf`），标准库 `/proc` 文件读取。

---

## File Structure

- **Create** `common/src/process_status.rs` — 跨平台 PID 文件探测 + 进程细节（uptime/rss）。纯函数解析逻辑可单测。
- **Modify** `common/src/lib.rs` — 注册 `pub mod process_status;`。
- **Modify** `common/src/systemd.rs` — 新增 `query_service()` + `SystemdStatus` 结构（Linux only）。
- **Modify** `controller/src/main.rs` — 加 `Status` CLI 变体、两个 main 的 dispatch 分支、`print_status()` 封装。
- **Modify** `node/src/main.rs` — 同上。
- **Modify** `client/src/main.rs` — 同上。

---

## Task 1: `common` 进程探测模块（核心，含单测）

**Files:**
- Create: `common/src/process_status.rs`
- Modify: `common/src/lib.rs`（加 `pub mod process_status;`）
- Test: 内联 `#[cfg(test)]` 于 `common/src/process_status.rs`

- [ ] **Step 1: 写失败的测试**

在新文件 `common/src/process_status.rs` 末尾先放测试（解析纯函数尚未实现，编译应失败）：

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_starttime_handles_comm_with_spaces_and_parens() {
        // /proc/<pid>/stat：第 2 字段 comm 被括号包裹，可能含空格和右括号。
        // 必须从最后一个 ')' 之后再按空格切分。starttime 是 ')' 之后的第 20 个字段。
        // 构造：pid=123, comm="(my proc)", state=S, 后续字段填充到 starttime=88997。
        // ')' 之后字段索引（1 起）：1=state ... 20=starttime
        let after = "S 1 1 1 0 -1 0 0 0 0 0 0 0 0 0 0 0 0 88997 0";
        let stat = format!("123 (my proc) {}", after);
        assert_eq!(parse_starttime_ticks(&stat), Some(88997));
    }

    #[test]
    fn parse_starttime_returns_none_on_garbage() {
        assert_eq!(parse_starttime_ticks("not a stat line"), None);
    }

    #[test]
    fn parse_vmrss_reads_kb() {
        let status = "Name:\tnode\nVmRSS:\t  46284 kB\nThreads:\t12\n";
        assert_eq!(parse_vmrss_kb(status), Some(46284));
    }

    #[test]
    fn parse_vmrss_returns_none_when_absent() {
        assert_eq!(parse_vmrss_kb("Name:\tnode\nThreads:\t12\n"), None);
    }
}
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test -p common process_status 2>&1 | head -30`
Expected: 编译错误，`cannot find function parse_starttime_ticks` / `parse_vmrss_kb`。

- [ ] **Step 3: 写实现**

在 `common/src/process_status.rs` 顶部（测试模块之前）写：

```rust
//! 跨平台进程状态探测。
//!
//! 通过 PID 文件判断守护进程是否运行，并在 Linux 上读取 `/proc`
//! 获取运行时长与内存占用（零额外依赖）。Windows 仅做存活探测。

use std::fs;

/// 进程运行细节。
#[derive(Debug, Clone)]
pub struct ProcessDetails {
    pub pid: i32,
    /// 进程已运行秒数。无法获取（如 Windows）时为 None。
    pub uptime_secs: Option<u64>,
    /// 常驻内存字节数（RSS）。无法获取时为 None。
    pub rss_bytes: Option<u64>,
}

/// PID 文件探测结果。
#[derive(Debug, Clone)]
pub enum PidFileStatus {
    /// PID 文件不存在。
    NoFile,
    /// PID 文件存在但解析失败（内容损坏）。
    Invalid(String),
    /// PID 文件存在，但进程已不在。残留的 PID 值随附。
    Stale(i32),
    /// 进程存活。
    Running(ProcessDetails),
}

/// 读取并探测 PID 文件。
pub fn probe_pid_file(pid_file: &str) -> PidFileStatus {
    let content = match fs::read_to_string(pid_file) {
        Ok(c) => c,
        Err(_) => return PidFileStatus::NoFile,
    };
    let pid: i32 = match content.trim().parse() {
        Ok(p) => p,
        Err(e) => return PidFileStatus::Invalid(format!("PID 文件内容无效: {}", e)),
    };

    if !is_alive(pid) {
        return PidFileStatus::Stale(pid);
    }

    PidFileStatus::Running(ProcessDetails {
        pid,
        uptime_secs: read_uptime_secs(pid),
        rss_bytes: read_rss_bytes(pid),
    })
}

// ─── 存活探测 ─────────────────────────────────────────────

#[cfg(unix)]
fn is_alive(pid: i32) -> bool {
    // kill(pid, 0)：不发送信号，仅检查进程是否存在且有权限。
    // 返回 0 表示存活；ESRCH 表示不存在；EPERM 表示存在但无权限（仍视为存活）。
    let ret = unsafe { libc::kill(pid, 0) };
    if ret == 0 {
        return true;
    }
    std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

#[cfg(windows)]
fn is_alive(pid: i32) -> bool {
    // Windows 无 /proc，用 tasklist 过滤 PID 判断是否存在。
    use std::process::Command;
    let out = Command::new("tasklist")
        .args(["/FI", &format!("PID eq {}", pid), "/NH", "/FO", "CSV"])
        .output();
    match out {
        Ok(o) => {
            let s = String::from_utf8_lossy(&o.stdout);
            // 存在时输出含该 PID 的 CSV 行；不存在时输出 "信息: 没有运行的任务..."。
            s.contains(&format!("\"{}\"", pid))
        }
        Err(_) => false,
    }
}

// ─── Linux /proc 读取 ─────────────────────────────────────

#[cfg(target_os = "linux")]
fn read_uptime_secs(pid: i32) -> Option<u64> {
    let stat = fs::read_to_string(format!("/proc/{}/stat", pid)).ok()?;
    let starttime_ticks = parse_starttime_ticks(&stat)?;

    let system_uptime = fs::read_to_string("/proc/uptime").ok()?;
    let system_uptime_secs: f64 = system_uptime.split_whitespace().next()?.parse().ok()?;

    let clk_tck = unsafe { libc::sysconf(libc::_SC_CLK_TCK) };
    if clk_tck <= 0 {
        return None;
    }
    let proc_start_secs = starttime_ticks as f64 / clk_tck as f64;
    let uptime = system_uptime_secs - proc_start_secs;
    if uptime < 0.0 {
        Some(0)
    } else {
        Some(uptime as u64)
    }
}

#[cfg(target_os = "linux")]
fn read_rss_bytes(pid: i32) -> Option<u64> {
    let status = fs::read_to_string(format!("/proc/{}/status", pid)).ok()?;
    parse_vmrss_kb(&status).map(|kb| kb * 1024)
}

#[cfg(not(target_os = "linux"))]
fn read_uptime_secs(_pid: i32) -> Option<u64> {
    None
}

#[cfg(not(target_os = "linux"))]
fn read_rss_bytes(_pid: i32) -> Option<u64> {
    None
}

// ─── 纯解析函数（可单测，不依赖 cfg） ──────────────────────

/// 从 `/proc/<pid>/stat` 提取 starttime（字段 22，单位 clock ticks）。
///
/// 第 2 字段 comm 被括号包裹，可能含空格甚至右括号，因此先定位到最后一个
/// `)`，从其后按空格切分。`)` 之后第 1 个 token 是 state（字段 3），
/// starttime 是 `)` 之后的第 20 个 token。
fn parse_starttime_ticks(stat: &str) -> Option<u64> {
    let close = stat.rfind(')')?;
    let rest = stat[close + 1..].trim();
    // rest 的 token：索引 0 = state(字段3) ... starttime 是字段 22 = 索引 19。
    rest.split_whitespace().nth(19)?.parse().ok()
}

/// 从 `/proc/<pid>/status` 提取 VmRSS（单位 kB）。
fn parse_vmrss_kb(status: &str) -> Option<u64> {
    for line in status.lines() {
        if let Some(rest) = line.strip_prefix("VmRSS:") {
            // 形如 "VmRSS:\t  46284 kB"
            return rest.split_whitespace().next()?.parse().ok();
        }
    }
    None
}
```

然后在 `common/src/lib.rs` 第 10 行（`pub mod grpc;`）之后加一行：

```rust
pub mod process_status;
```

- [ ] **Step 4: 运行测试确认通过**

Run: `cargo test -p common process_status 2>&1 | tail -15`
Expected: 4 个测试 PASS。

- [ ] **Step 5: 提交**

```bash
git add common/src/process_status.rs common/src/lib.rs
git commit -m "feat(common): 新增跨平台进程状态探测模块"
```

---

## Task 2: `common::systemd` 服务状态查询

**Files:**
- Modify: `common/src/systemd.rs`（在 `uninstall_service` 之后、`fn unit_path` 之前插入）

- [ ] **Step 1: 写实现（该函数依赖外部 systemctl，不做单测，仅保证编译）**

在 `common/src/systemd.rs` 的 `uninstall_service` 函数结束（第 90 行 `}` 之后）插入：

```rust
/// systemd 服务运行状态查询结果。
#[derive(Debug, Clone)]
pub struct SystemdStatus {
    /// `systemctl is-active` 的输出（如 "active" / "inactive" / "failed"）。
    pub active: String,
    /// `systemctl is-enabled` 的输出（如 "enabled" / "disabled" / "static"）。
    pub enabled: String,
    /// systemctl 是否可用。不可用时上面两个字段为占位值。
    pub systemctl_available: bool,
}

/// 查询 systemd 服务的 active / enabled 状态。
///
/// 不要求 root；`systemctl is-active` 任何用户都能查。
/// 即使服务不存在 systemctl 也会返回非零退出码 + "inactive"/"unknown" 文本，
/// 这里捕获 stdout 文本而不把非零退出码当错误。
pub fn query_service(service_name: &str) -> SystemdStatus {
    let unit = format!("{}.service", service_name);

    let probe = |subcmd: &str| -> Option<String> {
        let out = Command::new("systemctl").args([subcmd, &unit]).output().ok()?;
        let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if text.is_empty() {
            // is-enabled 对不存在的服务可能把信息写到 stderr
            let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
            if err.is_empty() {
                Some("unknown".to_string())
            } else {
                Some(err)
            }
        } else {
            Some(text)
        }
    };

    let active = probe("is-active");
    let enabled = probe("is-enabled");

    match (active, enabled) {
        (Some(a), Some(e)) => SystemdStatus {
            active: a,
            enabled: e,
            systemctl_available: true,
        },
        _ => SystemdStatus {
            active: "unknown".to_string(),
            enabled: "unknown".to_string(),
            systemctl_available: false,
        },
    }
}
```

- [ ] **Step 2: 编译确认通过**

Run: `cargo build -p common 2>&1 | tail -15`
Expected: 编译成功，无 warning 关于未使用项（`query_service`/`SystemdStatus` 是 `pub`，不会触发 dead_code）。

- [ ] **Step 3: 提交**

```bash
git add common/src/systemd.rs
git commit -m "feat(common): systemd 服务状态查询函数 query_service"
```

---

## Task 3: Controller `status` 命令

**Files:**
- Modify: `controller/src/main.rs`（CLI 变体 ~第 88 行后；非 windows main dispatch ~第 138 行；windows main dispatch ~第 270 行；新增 `print_status` 函数）

- [ ] **Step 1: 加 CLI 变体**

在 `controller/src/main.rs` 的 `enum Command` 中，`Update` 变体（第 88-89 行 `/// 更新到最新版本\n    Update,`）之后插入：

```rust
    /// 查询守护进程运行状态
    Status {
        /// PID 文件路径
        #[cfg(unix)]
        #[arg(long, default_value = "/var/run/oxiproxy-controller.pid")]
        pid_file: String,

        /// PID 文件路径
        #[cfg(windows)]
        #[arg(long, default_value = "oxiproxy-controller.pid")]
        pid_file: String,

        /// systemd 服务名（仅 Linux）
        #[cfg(target_os = "linux")]
        #[arg(long, default_value = "oxiproxy-controller")]
        service_name: String,
    },
```

- [ ] **Step 2: 加 `print_status` 函数**

在 `controller/src/main.rs` 的 `stop_daemon_unix` 函数（第 262 行 `}` 结束）之后插入。注意此函数无 cfg 限制，两个平台的 main 都调用它；systemd 段用 `#[cfg(target_os = "linux")]` 内联保护：

```rust
/// 打印守护进程状态并返回退出码（0=运行中/active，1=未运行）。
fn print_status(pid_file: &str, #[cfg(target_os = "linux")] service_name: &str) -> i32 {
    use common::process_status::{probe_pid_file, PidFileStatus};

    let mut healthy = false;

    println!("守护进程 (PID 文件):");
    println!("  PID 文件:   {}", pid_file);
    match probe_pid_file(pid_file) {
        PidFileStatus::NoFile => {
            println!("  状态:       未运行 (无 PID 文件)");
        }
        PidFileStatus::Invalid(msg) => {
            println!("  状态:       异常 ({})", msg);
        }
        PidFileStatus::Stale(pid) => {
            println!("  状态:       已停止 (PID 文件残留: {})", pid);
        }
        PidFileStatus::Running(d) => {
            healthy = true;
            println!("  状态:       运行中 (PID: {})", d.pid);
            match d.uptime_secs {
                Some(s) => println!("  运行时长:   {}", format_uptime(s)),
                None => println!("  运行时长:   不可用"),
            }
            match d.rss_bytes {
                Some(b) => println!("  内存占用:   {}", format_bytes(b)),
                None => println!("  内存占用:   不可用"),
            }
        }
    }

    #[cfg(target_os = "linux")]
    {
        let s = common::systemd::query_service(service_name);
        println!();
        println!("systemd 服务 ({}):", service_name);
        if s.systemctl_available {
            println!("  active:     {}", s.active);
            println!("  enabled:    {}", s.enabled);
            if s.active == "active" {
                healthy = true;
            }
        } else {
            println!("  systemctl 不可用，跳过");
        }
    }

    if healthy {
        0
    } else {
        1
    }
}

/// 把秒数格式化为 "Xd Yh Zm Ws" 风格（中文单位）。
fn format_uptime(mut secs: u64) -> String {
    let days = secs / 86400;
    secs %= 86400;
    let hours = secs / 3600;
    secs %= 3600;
    let mins = secs / 60;
    secs %= 60;
    let mut parts = Vec::new();
    if days > 0 {
        parts.push(format!("{}天", days));
    }
    if hours > 0 {
        parts.push(format!("{}小时", hours));
    }
    if mins > 0 {
        parts.push(format!("{}分钟", mins));
    }
    parts.push(format!("{}秒", secs));
    parts.join("")
}

/// 把字节数格式化为人类可读单位。
fn format_bytes(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = 1024.0 * 1024.0;
    const GB: f64 = 1024.0 * 1024.0 * 1024.0;
    let b = bytes as f64;
    if b >= GB {
        format!("{:.2} GB", b / GB)
    } else if b >= MB {
        format!("{:.2} MB", b / MB)
    } else if b >= KB {
        format!("{:.2} KB", b / KB)
    } else {
        format!("{} B", bytes)
    }
}
```

- [ ] **Step 3: 接入非 windows main dispatch**

在 `controller/src/main.rs` 的 `#[cfg(not(windows))] fn main()`（第 135 行起）的 match 中，`Command::Passwd` 分支（约第 285 行）之前插入。注意非 Linux 的 Unix（如 macOS）没有 `service_name` 字段：

```rust
        #[cfg(target_os = "linux")]
        Command::Status {
            pid_file,
            service_name,
        } => {
            let code = print_status(&pid_file, &service_name);
            std::process::exit(code);
        }

        #[cfg(all(unix, not(target_os = "linux")))]
        Command::Status { pid_file } => {
            let code = print_status(&pid_file);
            std::process::exit(code);
        }
```

- [ ] **Step 4: 接入 windows main dispatch**

在 `controller/src/main.rs` 的 `#[cfg(windows)] fn main()`（第 267 行起）的 match 中，`Command::Passwd`（第 285 行）之前插入：

```rust
        Command::Status { pid_file } => {
            let code = print_status(&pid_file);
            std::process::exit(code);
        }
```

- [ ] **Step 5: 编译并手动验证**

Run: `cargo build -p controller 2>&1 | tail -15`
Expected: 编译成功。

Run: `cargo run -q -p controller -- status --pid-file /tmp/nonexist.pid; echo "exit=$?"`
Expected: 打印「未运行 (无 PID 文件)」+ systemd 段（本机 happy 是 LXC，有 systemd，会显示 `oxiproxy-controller` 服务 inactive/unknown），最后 `exit=1`。

- [ ] **Step 6: 提交**

```bash
git add controller/src/main.rs
git commit -m "feat(controller): 新增 status 命令查询运行状态"
```

---

## Task 4: Node `status` 命令

**Files:**
- Modify: `node/src/main.rs`（CLI 变体 ~第 96 行后；非 windows main dispatch ~第 231 行；windows main dispatch ~第 389 行；新增 `print_status` 等函数）

- [ ] **Step 1: 加 CLI 变体**

在 `node/src/main.rs` 的 `enum Command` 中，`Update`（第 95-96 行 `/// 更新到最新版本\n    Update,`）之后插入：

```rust
    /// 查询守护进程运行状态
    Status {
        /// PID 文件路径
        #[cfg(unix)]
        #[arg(long, default_value = "/var/run/oxiproxy-node.pid")]
        pid_file: String,

        /// PID 文件路径
        #[cfg(windows)]
        #[arg(long, default_value = "oxiproxy-node.pid")]
        pid_file: String,

        /// systemd 服务名（仅 Linux）
        #[cfg(target_os = "linux")]
        #[arg(long, default_value = "oxiproxy-node")]
        service_name: String,
    },
```

- [ ] **Step 2: 加 `print_status` + 格式化函数**

在 `node/src/main.rs` 的 `stop_daemon_unix` 函数（第 320 行 `#[cfg(unix)]` 起的函数）结束之后插入。函数体与 Controller 完全一致，仅 service_name 默认值不同（已在 CLI 处理）：

```rust
/// 打印守护进程状态并返回退出码（0=运行中/active，1=未运行）。
fn print_status(pid_file: &str, #[cfg(target_os = "linux")] service_name: &str) -> i32 {
    use common::process_status::{probe_pid_file, PidFileStatus};

    let mut healthy = false;

    println!("守护进程 (PID 文件):");
    println!("  PID 文件:   {}", pid_file);
    match probe_pid_file(pid_file) {
        PidFileStatus::NoFile => {
            println!("  状态:       未运行 (无 PID 文件)");
        }
        PidFileStatus::Invalid(msg) => {
            println!("  状态:       异常 ({})", msg);
        }
        PidFileStatus::Stale(pid) => {
            println!("  状态:       已停止 (PID 文件残留: {})", pid);
        }
        PidFileStatus::Running(d) => {
            healthy = true;
            println!("  状态:       运行中 (PID: {})", d.pid);
            match d.uptime_secs {
                Some(s) => println!("  运行时长:   {}", format_uptime(s)),
                None => println!("  运行时长:   不可用"),
            }
            match d.rss_bytes {
                Some(b) => println!("  内存占用:   {}", format_bytes(b)),
                None => println!("  内存占用:   不可用"),
            }
        }
    }

    #[cfg(target_os = "linux")]
    {
        let s = common::systemd::query_service(service_name);
        println!();
        println!("systemd 服务 ({}):", service_name);
        if s.systemctl_available {
            println!("  active:     {}", s.active);
            println!("  enabled:    {}", s.enabled);
            if s.active == "active" {
                healthy = true;
            }
        } else {
            println!("  systemctl 不可用，跳过");
        }
    }

    if healthy {
        0
    } else {
        1
    }
}

/// 把秒数格式化为中文单位的运行时长。
fn format_uptime(mut secs: u64) -> String {
    let days = secs / 86400;
    secs %= 86400;
    let hours = secs / 3600;
    secs %= 3600;
    let mins = secs / 60;
    secs %= 60;
    let mut parts = Vec::new();
    if days > 0 {
        parts.push(format!("{}天", days));
    }
    if hours > 0 {
        parts.push(format!("{}小时", hours));
    }
    if mins > 0 {
        parts.push(format!("{}分钟", mins));
    }
    parts.push(format!("{}秒", secs));
    parts.join("")
}

/// 把字节数格式化为人类可读单位。
fn format_bytes(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = 1024.0 * 1024.0;
    const GB: f64 = 1024.0 * 1024.0 * 1024.0;
    let b = bytes as f64;
    if b >= GB {
        format!("{:.2} GB", b / GB)
    } else if b >= MB {
        format!("{:.2} MB", b / MB)
    } else if b >= KB {
        format!("{:.2} KB", b / KB)
    } else {
        format!("{} B", bytes)
    }
}
```

- [ ] **Step 3: 接入非 windows main dispatch**

在 `node/src/main.rs` 的 `#[cfg(not(windows))] fn main()`（第 164 行起）的 match 中，`Command::Uninstall { service_name }` 分支（第 259 行）之后插入：

```rust
        #[cfg(target_os = "linux")]
        Command::Status {
            pid_file,
            service_name,
        } => {
            let code = print_status(&pid_file, &service_name);
            std::process::exit(code);
        }

        #[cfg(all(unix, not(target_os = "linux")))]
        Command::Status { pid_file } => {
            let code = print_status(&pid_file);
            std::process::exit(code);
        }
```

- [ ] **Step 4: 接入 windows main dispatch**

在 `node/src/main.rs` 的 `#[cfg(windows)] fn main()`（第 348 行起）的 match 中，`Command::Update => update_binary(),`（第 389 行）之后插入：

```rust
        Command::Status { pid_file } => {
            let code = print_status(&pid_file);
            std::process::exit(code);
        }
```

- [ ] **Step 5: 编译并手动验证**

Run: `cargo build -p node 2>&1 | tail -15`
Expected: 编译成功。

Run: `cargo run -q -p node -- status --pid-file /tmp/nonexist.pid; echo "exit=$?"`
Expected: 打印「未运行 (无 PID 文件)」+ systemd `oxiproxy-node` 段，`exit=1`。

- [ ] **Step 6: 提交**

```bash
git add node/src/main.rs
git commit -m "feat(node): 新增 status 命令查询运行状态"
```

---

## Task 5: Client `status` 命令

**Files:**
- Modify: `client/src/main.rs`（CLI 变体 ~第 124 行 Update 后；非 windows main dispatch ~第 276 行后；windows main dispatch ~第 405 行后；新增 `print_status` 等函数）

- [ ] **Step 1: 加 CLI 变体**

在 `client/src/main.rs` 的 `enum Command` 中，`Update`（`/// 更新到最新版本\n    Update,`，约第 123-124 行）之后插入：

```rust
    /// 查询守护进程运行状态
    Status {
        /// PID 文件路径
        #[cfg(unix)]
        #[arg(long, default_value = "/var/run/oxiproxy-client.pid")]
        pid_file: String,

        /// PID 文件路径
        #[cfg(windows)]
        #[arg(long, default_value = "oxiproxy-client.pid")]
        pid_file: String,

        /// systemd 服务名（仅 Linux）
        #[cfg(target_os = "linux")]
        #[arg(long, default_value = "oxiproxy-client")]
        service_name: String,
    },
```

- [ ] **Step 2: 加 `print_status` + 格式化函数**

在 `client/src/main.rs` 的 `stop_daemon_unix` 函数（`#[cfg(unix)]`，约第 334 行起）结束之后插入。函数体与 Node 完全一致（service_name 默认值差异已在 CLI 处理）：

```rust
/// 打印守护进程状态并返回退出码（0=运行中/active，1=未运行）。
fn print_status(pid_file: &str, #[cfg(target_os = "linux")] service_name: &str) -> i32 {
    use common::process_status::{probe_pid_file, PidFileStatus};

    let mut healthy = false;

    println!("守护进程 (PID 文件):");
    println!("  PID 文件:   {}", pid_file);
    match probe_pid_file(pid_file) {
        PidFileStatus::NoFile => {
            println!("  状态:       未运行 (无 PID 文件)");
        }
        PidFileStatus::Invalid(msg) => {
            println!("  状态:       异常 ({})", msg);
        }
        PidFileStatus::Stale(pid) => {
            println!("  状态:       已停止 (PID 文件残留: {})", pid);
        }
        PidFileStatus::Running(d) => {
            healthy = true;
            println!("  状态:       运行中 (PID: {})", d.pid);
            match d.uptime_secs {
                Some(s) => println!("  运行时长:   {}", format_uptime(s)),
                None => println!("  运行时长:   不可用"),
            }
            match d.rss_bytes {
                Some(b) => println!("  内存占用:   {}", format_bytes(b)),
                None => println!("  内存占用:   不可用"),
            }
        }
    }

    #[cfg(target_os = "linux")]
    {
        let s = common::systemd::query_service(service_name);
        println!();
        println!("systemd 服务 ({}):", service_name);
        if s.systemctl_available {
            println!("  active:     {}", s.active);
            println!("  enabled:    {}", s.enabled);
            if s.active == "active" {
                healthy = true;
            }
        } else {
            println!("  systemctl 不可用，跳过");
        }
    }

    if healthy {
        0
    } else {
        1
    }
}

/// 把秒数格式化为中文单位的运行时长。
fn format_uptime(mut secs: u64) -> String {
    let days = secs / 86400;
    secs %= 86400;
    let hours = secs / 3600;
    secs %= 3600;
    let mins = secs / 60;
    secs %= 60;
    let mut parts = Vec::new();
    if days > 0 {
        parts.push(format!("{}天", days));
    }
    if hours > 0 {
        parts.push(format!("{}小时", hours));
    }
    if mins > 0 {
        parts.push(format!("{}分钟", mins));
    }
    parts.push(format!("{}秒", secs));
    parts.join("")
}

/// 把字节数格式化为人类可读单位。
fn format_bytes(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = 1024.0 * 1024.0;
    const GB: f64 = 1024.0 * 1024.0 * 1024.0;
    let b = bytes as f64;
    if b >= GB {
        format!("{:.2} GB", b / GB)
    } else if b >= MB {
        format!("{:.2} MB", b / MB)
    } else if b >= KB {
        format!("{:.2} KB", b / KB)
    } else {
        format!("{} B", bytes)
    }
}
```

- [ ] **Step 3: 接入非 windows main dispatch**

在 `client/src/main.rs` 的 `#[cfg(not(windows))] fn main()`（第 184 行起）的 match 中，`Command::Uninstall { service_name }` 分支（第 276 行）之后插入：

```rust
        #[cfg(target_os = "linux")]
        Command::Status {
            pid_file,
            service_name,
        } => {
            let code = print_status(&pid_file, &service_name);
            std::process::exit(code);
        }

        #[cfg(all(unix, not(target_os = "linux")))]
        Command::Status { pid_file } => {
            let code = print_status(&pid_file);
            std::process::exit(code);
        }
```

- [ ] **Step 4: 接入 windows main dispatch**

在 `client/src/main.rs` 的 `#[cfg(windows)] fn main()`（第 363 行起）的 match 中，`Command::Update => update_binary(),`（第 405 行）之后插入：

```rust
        Command::Status { pid_file } => {
            let code = print_status(&pid_file);
            std::process::exit(code);
        }
```

- [ ] **Step 5: 编译并手动验证**

Run: `cargo build -p client 2>&1 | tail -15`
Expected: 编译成功。

Run: `cargo run -q -p client -- status --pid-file /tmp/nonexist.pid; echo "exit=$?"`
Expected: 打印「未运行 (无 PID 文件)」+ systemd `oxiproxy-client` 段，`exit=1`。

- [ ] **Step 6: 提交**

```bash
git add client/src/main.rs
git commit -m "feat(client): 新增 status 命令查询运行状态"
```

---

## Task 6: 全量校验 + 文档

**Files:**
- Modify: `CLAUDE.md`（更新三个组件的子命令说明，提及 `status`）

- [ ] **Step 1: 全量构建 + clippy + 测试**

Run: `cargo build --release 2>&1 | tail -5 && cargo test -p common 2>&1 | tail -10 && cargo clippy --all-targets --all-features -- -D warnings 2>&1 | tail -20`
Expected: 全部成功，clippy 无 warning。

- [ ] **Step 2: 端到端验证「运行中」场景**

启动一个 controller daemon，再查 status，确认运行时长/内存非空：

```bash
cargo run -q -p controller -- daemon --pid-file /tmp/ctl.pid --log-dir /tmp/ctl-logs
sleep 2
cargo run -q -p controller -- status --pid-file /tmp/ctl.pid; echo "exit=$?"
cargo run -q -p controller -- stop --pid-file /tmp/ctl.pid
```

Expected: status 显示「运行中 (PID: …)」、运行时长「2秒」级别、内存「XX MB」，`exit=0`。

- [ ] **Step 3: 更新 CLAUDE.md**

在 `CLAUDE.md` 的 Client/Node/Controller 模块描述中，把 `status` 加入各自子命令列表。具体改三处 `main.rs` 的说明行：

- Controller `main.rs` 描述行：`CLI 子命令：start、daemon、stop、update`（如有）→ 加 `status`
- Node `main.rs` 行第 — `CLI 子命令解析（clap）：start、daemon、stop、update` → 改为 `start、daemon、stop、status、update`
- Client `main.rs` 行 — `CLI 子命令：start、daemon、stop、update` → 改为 `start、daemon、stop、status、update`

（按文件实际文案做最小替换，确保提到 `status` 即可。）

- [ ] **Step 4: 提交**

```bash
git add CLAUDE.md
git commit -m "docs: 补充 status 命令到组件说明"
```

---

## Self-Review 结论

- **Spec 覆盖**：PID 探测（Task 1）、systemd 查询（Task 2）、三组件命令（Task 3-5）、运行细节 `/proc`（Task 1）、退出码（print_status 返回值 + `std::process::exit`）、零依赖（仅用已有 libc + std）、Windows 简化（`read_*` 在非 Linux 返回 None、`is_alive` 走 tasklist）—— 全部有对应任务。
- **占位符**：无。每个 step 含完整代码或确切命令。
- **类型一致**：`PidFileStatus` / `ProcessDetails` / `SystemdStatus` 字段名在 common 定义后，print_status 中引用一致；`probe_pid_file` / `query_service` / `format_uptime` / `format_bytes` 签名贯穿一致。
- **已知权衡**：`print_status` + 两个格式化函数在三个 main 中各复制一份（共 3 份）。可下沉到 common，但会牵扯 `#[cfg]` 跨 crate 传 service_name 的别扭，且三处 main 本就各有平台 dispatch，保持局部更简单——遵循 YAGNI，不过度抽象。
