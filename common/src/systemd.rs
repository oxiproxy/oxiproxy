//! systemd 服务安装/卸载工具（仅 Linux）
//!
//! 提供将 OxiProxy 二进制注册为 systemd 服务的能力，
//! 实现 enable + start 一步到位的开机自启。

#![cfg(target_os = "linux")]

use anyhow::{anyhow, Result};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// systemd 服务定义
pub struct SystemdServiceConfig {
    /// 服务名（不含 .service 后缀），例如 "oxiproxy-node"
    pub service_name: String,
    /// Unit Description
    pub description: String,
    /// 二进制绝对路径
    pub binary_path: PathBuf,
    /// 启动参数（不含二进制本身）
    pub args: Vec<String>,
    /// 工作目录
    pub working_dir: PathBuf,
    /// 运行用户，None 表示 root
    pub user: Option<String>,
}

/// systemd unit 文件目录
const UNIT_DIR: &str = "/etc/systemd/system";

/// 安装并启用服务（写 unit 文件 + daemon-reload + enable --now）
pub fn install_service(config: &SystemdServiceConfig) -> Result<()> {
    ensure_root()?;
    ensure_systemctl()?;

    let unit_path = unit_path(&config.service_name);
    let unit_content = render_unit(config);

    if let Some(parent) = unit_path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| anyhow!("无法创建 systemd 目录 {}: {}", parent.display(), e))?;
    }

    fs::write(&unit_path, unit_content)
        .map_err(|e| anyhow!("写入 unit 文件 {} 失败: {}", unit_path.display(), e))?;
    println!("✅ 已写入 unit 文件: {}", unit_path.display());

    run_systemctl(&["daemon-reload"])?;
    // 已存在则先停止，避免旧实例继续占用资源
    let _ = run_systemctl(&["stop", &format!("{}.service", config.service_name)]);
    run_systemctl(&["enable", "--now", &format!("{}.service", config.service_name)])?;

    println!();
    println!("🚀 服务已启动并设置为开机自启: {}", config.service_name);
    println!();
    println!("常用命令：");
    println!("  查看状态:  systemctl status {}", config.service_name);
    println!("  查看日志:  journalctl -u {} -f", config.service_name);
    println!("  停止服务:  systemctl stop {}", config.service_name);
    println!("  禁用自启:  systemctl disable {}", config.service_name);

    Ok(())
}

/// 卸载服务（disable + stop + 删除 unit 文件 + daemon-reload）
pub fn uninstall_service(service_name: &str) -> Result<()> {
    ensure_root()?;
    ensure_systemctl()?;

    let unit_path = unit_path(service_name);
    let unit_name = format!("{}.service", service_name);

    // 即便服务已停或不存在也继续往下走
    let _ = run_systemctl(&["disable", "--now", &unit_name]);

    if unit_path.exists() {
        fs::remove_file(&unit_path)
            .map_err(|e| anyhow!("删除 unit 文件 {} 失败: {}", unit_path.display(), e))?;
        println!("✅ 已删除 unit 文件: {}", unit_path.display());
    } else {
        println!("ℹ️  unit 文件不存在: {}", unit_path.display());
    }

    run_systemctl(&["daemon-reload"])?;
    let _ = run_systemctl(&["reset-failed", &unit_name]);

    println!("🧹 服务已卸载: {}", service_name);
    Ok(())
}

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

fn unit_path(service_name: &str) -> PathBuf {
    Path::new(UNIT_DIR).join(format!("{}.service", service_name))
}

fn ensure_root() -> Result<()> {
    let euid = unsafe { libc::geteuid() };
    if euid != 0 {
        return Err(anyhow!(
            "此操作需要 root 权限（当前 euid={}），请使用 sudo 运行",
            euid
        ));
    }
    Ok(())
}

fn ensure_systemctl() -> Result<()> {
    let status = Command::new("systemctl").arg("--version").output();
    match status {
        Ok(out) if out.status.success() => Ok(()),
        Ok(out) => Err(anyhow!(
            "systemctl 不可用：{}",
            String::from_utf8_lossy(&out.stderr)
        )),
        Err(e) => Err(anyhow!(
            "未找到 systemctl，请确认系统是否使用 systemd: {}",
            e
        )),
    }
}

fn run_systemctl(args: &[&str]) -> Result<()> {
    let output = Command::new("systemctl")
        .args(args)
        .output()
        .map_err(|e| anyhow!("执行 systemctl 失败: {}", e))?;

    if !output.status.success() {
        return Err(anyhow!(
            "systemctl {} 失败: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(())
}

fn render_unit(config: &SystemdServiceConfig) -> String {
    let exec_start = build_exec_start(&config.binary_path, &config.args);
    let user_line = match &config.user {
        Some(u) => format!("User={}\n", u),
        None => String::new(),
    };

    format!(
        "[Unit]\n\
Description={description}\n\
After=network-online.target\n\
Wants=network-online.target\n\
\n\
[Service]\n\
Type=simple\n\
ExecStart={exec_start}\n\
WorkingDirectory={working_dir}\n\
{user_line}Restart=on-failure\n\
RestartSec=5s\n\
LimitNOFILE=65536\n\
StandardOutput=journal\n\
StandardError=journal\n\
\n\
[Install]\n\
WantedBy=multi-user.target\n",
        description = config.description,
        exec_start = exec_start,
        working_dir = config.working_dir.display(),
        user_line = user_line,
    )
}

fn build_exec_start(binary: &Path, args: &[String]) -> String {
    let mut parts = Vec::with_capacity(args.len() + 1);
    parts.push(quote_arg(&binary.display().to_string()));
    for arg in args {
        parts.push(quote_arg(arg));
    }
    parts.join(" ")
}

/// 对 systemd ExecStart 参数做最小化转义
///
/// systemd 支持双引号包裹的参数，引号内可用 \" / \\ / \$ 转义。
/// 仅对包含空格或特殊字符的参数加引号，保持 unit 文件可读性。
fn quote_arg(arg: &str) -> String {
    let needs_quote = arg.is_empty()
        || arg
            .chars()
            .any(|c| !(c.is_ascii_alphanumeric() || "-_./:=@+,".contains(c)));
    if !needs_quote {
        return arg.to_string();
    }
    let escaped = arg
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('$', "\\$");
    format!("\"{}\"", escaped)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quote_arg_keeps_simple_strings() {
        assert_eq!(quote_arg("http://controller:3100"), "http://controller:3100");
        assert_eq!(quote_arg("abc"), "abc");
        assert_eq!(quote_arg("/var/log/oxiproxy"), "/var/log/oxiproxy");
    }

    #[test]
    fn quote_arg_quotes_special_chars() {
        assert_eq!(quote_arg("with space"), "\"with space\"");
        assert_eq!(quote_arg("a\"b"), "\"a\\\"b\"");
        assert_eq!(quote_arg("$VAR"), "\"\\$VAR\"");
        assert_eq!(quote_arg(""), "\"\"");
    }

    #[test]
    fn render_unit_contains_required_sections() {
        let cfg = SystemdServiceConfig {
            service_name: "oxiproxy-node".into(),
            description: "OxiProxy Node".into(),
            binary_path: PathBuf::from("/usr/local/bin/node"),
            args: vec!["start".into(), "--token".into(), "secret token".into()],
            working_dir: PathBuf::from("/var/lib/oxiproxy"),
            user: Some("root".into()),
        };
        let unit = render_unit(&cfg);
        assert!(unit.contains("[Unit]"));
        assert!(unit.contains("[Service]"));
        assert!(unit.contains("[Install]"));
        assert!(unit.contains("User=root"));
        assert!(unit.contains("WorkingDirectory=/var/lib/oxiproxy"));
        assert!(unit.contains("ExecStart=/usr/local/bin/node start --token \"secret token\""));
    }
}
