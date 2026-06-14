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
    rest.split_whitespace().nth(19)?.parse().ok()
}

/// 从 `/proc/<pid>/status` 提取 VmRSS（单位 kB）。
fn parse_vmrss_kb(status: &str) -> Option<u64> {
    for line in status.lines() {
        if let Some(rest) = line.strip_prefix("VmRSS:") {
            return rest.split_whitespace().next()?.parse().ok();
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_starttime_handles_comm_with_spaces_and_parens() {
        // /proc/<pid>/stat：第 2 字段 comm 被括号包裹，可能含空格和右括号。
        // 必须从最后一个 ')' 之后再按空格切分。starttime 是 ')' 之后的第 20 个字段。
        // ')' 之后的 token：state 是第 1 个，starttime 是第 20 个（索引 19）。
        // 与真实 /proc/<pid>/stat 字段布局一致（starttime 为整体第 22 字段）。
        let after = "S 1 1 1 0 -1 0 0 0 0 0 0 0 0 0 0 0 0 0 88997 0";
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
