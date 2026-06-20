mod client;

#[cfg(windows)]
mod windows_service;

use clap::{Parser, Subcommand};
use std::fs;

#[cfg(unix)]
use daemonize::Daemonize;
#[cfg(unix)]
use std::fs::File;

#[derive(Parser)]
#[command(name = "client", version, about = "OxiProxy Client - 反向代理客户端")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// 前台运行客户端
    Start {
        /// Controller 地址（例如 http://controller:3100）。
        /// 若已安装同名 systemd 服务则忽略，转为 systemctl start。
        #[arg(long)]
        controller_url: Option<String>,

        /// 客户端 Token。若已安装同名 systemd 服务则忽略。
        #[arg(long)]
        token: Option<String>,

        /// 自定义 CA 证书文件路径（PEM 格式，用于验证 Controller 的 TLS 证书）
        #[arg(long)]
        tls_ca_cert: Option<String>,

        /// 日志目录路径（按天自动分割，不指定则输出到控制台）
        #[arg(long)]
        log_dir: Option<String>,

        /// systemd 服务名（仅 Linux）：若该服务已安装则 start 转为 systemctl start
        #[cfg(target_os = "linux")]
        #[arg(long, default_value = "oxiproxy-client")]
        service_name: String,
    },

    /// 停止运行中的守护进程
    Stop {
        /// PID 文件路径
        #[cfg(unix)]
        #[arg(long, default_value = "/var/run/oxiproxy-client.pid")]
        pid_file: String,

        /// PID 文件路径
        #[cfg(windows)]
        #[arg(long, default_value = "oxiproxy-client.pid")]
        pid_file: String,

        /// systemd 服务名（仅 Linux）：若该服务已安装则 stop 转为 systemctl stop
        #[cfg(target_os = "linux")]
        #[arg(long, default_value = "oxiproxy-client")]
        service_name: String,
    },

    /// 以守护进程模式运行
    Daemon {
        /// Controller 地址（例如 http://controller:3100）
        #[arg(long)]
        controller_url: String,

        /// 客户端 Token
        #[arg(long)]
        token: String,

        /// 自定义 CA 证书文件路径（PEM 格式，用于验证 Controller 的 TLS 证书）
        #[arg(long)]
        tls_ca_cert: Option<String>,

        /// PID 文件路径
        #[cfg(unix)]
        #[arg(long, default_value = "/var/run/oxiproxy-client.pid")]
        pid_file: String,

        /// 日志目录路径（按天自动分割）
        #[cfg(unix)]
        #[arg(long, default_value = "./logs")]
        log_dir: String,

        /// PID 文件路径
        #[cfg(windows)]
        #[arg(long, default_value = "oxiproxy-client.pid")]
        pid_file: String,

        /// 日志目录路径（按天自动分割）
        #[cfg(windows)]
        #[arg(long, default_value = "./logs")]
        log_dir: String,
    },

    /// 安装为 Windows 服务（仅 Windows 系统）
    #[cfg(windows)]
    InstallService {
        /// Controller 地址（例如 http://controller:3100）
        #[arg(long)]
        controller_url: String,

        /// 客户端 Token
        #[arg(long)]
        token: String,

        /// 自定义 CA 证书文件路径（PEM 格式，用于验证 Controller 的 TLS 证书）
        #[arg(long)]
        tls_ca_cert: Option<String>,
    },

    /// 卸载 Windows 服务（仅 Windows 系统）
    #[cfg(windows)]
    UninstallService,

    /// 以 Windows 服务模式运行（由 SCM 调用，用户不应直接使用）
    #[cfg(windows)]
    #[command(hide = true)]
    Service {
        /// Controller 地址
        #[arg(long)]
        controller_url: Option<String>,

        /// 客户端 Token
        #[arg(long)]
        token: Option<String>,
    },

    /// 更新到最新版本
    Update,

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

    /// 查看本地日志（仅 daemon 模式落盘的日志；前台 start 模式日志只在终端）
    Log {
        /// 日志目录路径
        #[arg(long, default_value = "./logs")]
        log_dir: String,

        /// 打印末尾行数
        #[arg(short = 'n', long, default_value_t = 200)]
        lines: usize,

        /// 实时跟随（类似 tail -f），Ctrl-C 退出
        #[arg(short = 'f', long)]
        follow: bool,
    },

    /// 安装为 systemd 服务（开机自启，仅 Linux）
    #[cfg(target_os = "linux")]
    Install {
        /// Controller 地址
        #[arg(long)]
        controller_url: String,

        /// 客户端 Token
        #[arg(long)]
        token: String,

        /// 自定义 CA 证书文件路径
        #[arg(long)]
        tls_ca_cert: Option<String>,

        /// 日志目录路径（不指定则由 systemd-journald 收集）
        #[arg(long)]
        log_dir: Option<String>,

        /// systemd 服务名
        #[arg(long, default_value = "oxiproxy-client")]
        service_name: String,

        /// 运行服务的用户（默认 root）
        #[arg(long)]
        user: Option<String>,

        /// 工作目录（默认当前目录）
        #[arg(long)]
        working_dir: Option<String>,

        /// 已有 daemon 的 PID 文件路径（安装前会自动停止并清理该 daemon）
        #[arg(long, default_value = "/var/run/oxiproxy-client.pid")]
        pid_file: String,
    },

    /// 卸载 systemd 服务（仅 Linux）
    #[cfg(target_os = "linux")]
    Uninstall {
        /// systemd 服务名
        #[arg(long, default_value = "oxiproxy-client")]
        service_name: String,
    },
}

/// 加载 CA 证书文件内容
fn load_tls_ca_cert(path: &Option<String>) -> anyhow::Result<Option<Vec<u8>>> {
    match path {
        Some(p) => {
            let content = fs::read(p)
                .map_err(|e| anyhow::anyhow!("读取 CA 证书文件 {} 失败: {}", p, e))?;
            Ok(Some(content))
        }
        None => Ok(None),
    }
}

// ─── Unix 入口 ───────────────────────────────────────────
// 注意：不使用 #[tokio::main]，因为 daemon 模式需要在 fork 之后才创建 tokio runtime。
// 在 fork 之前创建的 runtime（epoll fd、worker 线程）会在 fork 后损坏，导致网络连接失败。

#[cfg(not(windows))]
fn main() -> anyhow::Result<()> {
    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("Failed to install rustls crypto provider");

    let cli = Cli::parse();

    match cli.command {
        Command::Start {
            controller_url,
            token,
            tls_ca_cert,
            log_dir,
            #[cfg(target_os = "linux")]
            service_name,
        } => {
            // 已安装同名 systemd 服务时，start 转为 systemctl start
            #[cfg(target_os = "linux")]
            if common::systemd::is_installed(&service_name) {
                println!(
                    "ℹ️  检测到已安装 systemd 服务 {}，转为 systemctl start",
                    service_name
                );
                common::systemd::start_service(&service_name)?;
                return Ok(());
            }
            // 前台运行：需要 controller_url 和 token
            let controller_url = controller_url.ok_or_else(|| {
                anyhow::anyhow!("未安装 systemd 服务，前台启动需 --controller-url 和 --token")
            })?;
            let token = token.ok_or_else(|| {
                anyhow::anyhow!("未安装 systemd 服务，前台启动需 --controller-url 和 --token")
            })?;
            let ca_cert = load_tls_ca_cert(&tls_ca_cert)?;
            if let Some(ref dir) = log_dir {
                fs::create_dir_all(dir).expect("无法创建日志目录");
            }
            let runtime = tokio::runtime::Runtime::new()?;
            runtime.block_on(client::run_client(controller_url, token, ca_cert, log_dir))?;
        }

        Command::Stop {
            pid_file,
            #[cfg(target_os = "linux")]
            service_name,
        } => {
            // 已安装同名 systemd 服务时，stop 转为 systemctl stop
            #[cfg(target_os = "linux")]
            if common::systemd::is_installed(&service_name) {
                println!(
                    "ℹ️  检测到已安装 systemd 服务 {}，转为 systemctl stop",
                    service_name
                );
                common::systemd::stop_service(&service_name)?;
                return Ok(());
            }
            stop_daemon_unix(&pid_file)?;
        }

        Command::Daemon {
            controller_url,
            token,
            tls_ca_cert,
            pid_file,
            log_dir,
        } => {
            // 确保日志目录存在
            fs::create_dir_all(&log_dir).expect("无法创建日志目录");

            println!("启动守护进程模式...");
            println!("PID 文件: {}", pid_file);
            println!("日志目录: {}", log_dir);

            // daemon 模式下 stdout/stderr 重定向到日志目录中的固定文件
            let stdout =
                File::create(format!("{}/daemon.log", log_dir)).expect("无法创建日志文件");
            let stderr =
                File::create(format!("{}/daemon.err", log_dir)).expect("无法创建错误日志文件");

            let daemonize = Daemonize::new()
                .pid_file(&pid_file)
                .working_directory(".")
                .stdout(stdout)
                .stderr(stderr);

            match daemonize.start() {
                Ok(_) => println!("守护进程已启动"),
                Err(e) => {
                    eprintln!("启动守护进程失败: {}", e);
                    std::process::exit(1);
                }
            }

            // fork 完成后再创建 tokio runtime，确保 epoll fd 和线程池状态正确
            let ca_cert = load_tls_ca_cert(&tls_ca_cert)?;
            let runtime = tokio::runtime::Runtime::new()?;
            runtime.block_on(client::run_client(controller_url, token, ca_cert, Some(log_dir)))?;
        }

        Command::Update => {
            update_binary()?;
        }

        #[cfg(target_os = "linux")]
        Command::Install {
            controller_url,
            token,
            tls_ca_cert,
            log_dir,
            service_name,
            user,
            working_dir,
            pid_file,
        } => {
            // install 优先级高于 daemon：先停掉并清理可能在跑的 daemon，避免两个实例并存。
            stop_daemon_if_running(&pid_file);
            install_systemd_service(
                controller_url,
                token,
                tls_ca_cert,
                log_dir,
                service_name,
                user,
                working_dir,
            )?;
        }

        #[cfg(target_os = "linux")]
        Command::Uninstall { service_name } => {
            common::systemd::uninstall_service(&service_name)?;
        }

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

        Command::Log { log_dir, lines, follow } => {
            let code = common::log_viewer::run(&log_dir, "client.log", lines, follow);
            std::process::exit(code);
        }
    }

    Ok(())
}

#[cfg(target_os = "linux")]
fn install_systemd_service(
    controller_url: String,
    token: String,
    tls_ca_cert: Option<String>,
    log_dir: Option<String>,
    service_name: String,
    user: Option<String>,
    working_dir: Option<String>,
) -> anyhow::Result<()> {
    use std::path::PathBuf;

    let binary_path = std::env::current_exe()?
        .canonicalize()
        .map_err(|e| anyhow::anyhow!("解析当前二进制路径失败: {}", e))?;

    let working_dir = match working_dir {
        Some(p) => PathBuf::from(p),
        None => std::env::current_dir()?,
    };

    let mut args = vec![
        "start".to_string(),
        "--controller-url".to_string(),
        controller_url,
        "--token".to_string(),
        token,
    ];
    if let Some(ca) = tls_ca_cert {
        args.push("--tls-ca-cert".to_string());
        args.push(ca);
    }
    if let Some(dir) = log_dir {
        fs::create_dir_all(&dir).ok();
        args.push("--log-dir".to_string());
        args.push(dir);
    }

    let config = common::systemd::SystemdServiceConfig {
        service_name,
        description: "OxiProxy Client".to_string(),
        binary_path,
        args,
        working_dir,
        user,
    };

    common::systemd::install_service(&config)
}

#[cfg(unix)]
fn stop_daemon_unix(pid_file: &str) -> anyhow::Result<()> {
    let pid_str = fs::read_to_string(pid_file)
        .map_err(|e| anyhow::anyhow!("无法读取 PID 文件 {}: {}", pid_file, e))?;
    let pid: i32 = pid_str
        .trim()
        .parse()
        .map_err(|e| anyhow::anyhow!("PID 文件内容无效: {}", e))?;

    let ret = unsafe { libc::kill(pid, libc::SIGTERM) };
    if ret != 0 {
        let err = std::io::Error::last_os_error();
        // ESRCH = no such process — already stopped
        if err.raw_os_error() == Some(libc::ESRCH) {
            println!("进程 (PID: {}) 已不存在", pid);
        } else {
            return Err(anyhow::anyhow!("停止进程失败 (PID: {}): {}", pid, err));
        }
    } else {
        println!("已发送停止信号到守护进程 (PID: {})", pid);
    }

    fs::remove_file(pid_file).ok();
    Ok(())
}

/// 安装 systemd 服务前的 best-effort 清理：若 PID 文件存在则停止旧 daemon。
///
/// install 优先级高于 daemon，因此这里任何失败都只告警、不中断安装，
/// 避免残留的 daemon PID 文件挡住 systemd 服务安装。
#[cfg(target_os = "linux")]
fn stop_daemon_if_running(pid_file: &str) {
    if !std::path::Path::new(pid_file).exists() {
        println!("ℹ️  未发现运行中的 daemon（{}），跳过", pid_file);
        return;
    }
    println!("🛑 检测到已有 daemon，安装前先停止：{}", pid_file);
    if let Err(e) = stop_daemon_unix(pid_file) {
        eprintln!("⚠️  停止旧 daemon 失败（继续安装）：{}", e);
        // 尽力清理 PID 文件，避免后续状态查询误判
        fs::remove_file(pid_file).ok();
    }
}

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

// ─── Windows 入口 ────────────────────────────────────────

#[cfg(windows)]
fn main() -> anyhow::Result<()> {
    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("Failed to install rustls crypto provider");

    let cli = Cli::parse();

    match cli.command {
        Command::Start {
            controller_url,
            token,
            tls_ca_cert,
            log_dir,
        } => {
            let controller_url = controller_url
                .ok_or_else(|| anyhow::anyhow!("前台启动需 --controller-url 和 --token"))?;
            let token = token
                .ok_or_else(|| anyhow::anyhow!("前台启动需 --controller-url 和 --token"))?;
            let ca_cert = load_tls_ca_cert(&tls_ca_cert)?;
            if let Some(ref dir) = log_dir {
                fs::create_dir_all(dir).expect("无法创建日志目录");
            }
            let runtime = tokio::runtime::Runtime::new()?;
            runtime.block_on(async { client::run_client(controller_url, token, ca_cert, log_dir).await })
        }

        Command::Stop { pid_file } => stop_daemon_windows(&pid_file),

        Command::Daemon {
            controller_url,
            token,
            tls_ca_cert,
            pid_file,
            log_dir,
        } => start_daemon_windows(&controller_url, &token, &tls_ca_cert, &pid_file, &log_dir),

        Command::InstallService {
            controller_url,
            token,
            tls_ca_cert,
        } => windows_service::install_service(&controller_url, &token, tls_ca_cert.as_deref()),

        Command::UninstallService => windows_service::uninstall_service(),

        Command::Service { .. } => windows_service::run_service(),

        Command::Update => update_binary(),

        Command::Status { pid_file } => {
            let code = print_status(&pid_file);
            std::process::exit(code);
        }

        Command::Log { log_dir, lines, follow } => {
            let code = common::log_viewer::run(&log_dir, "client.log", lines, follow);
            std::process::exit(code);
        }
    }
}

#[cfg(windows)]
fn start_daemon_windows(
    controller_url: &str,
    token: &str,
    tls_ca_cert: &Option<String>,
    pid_file: &str,
    log_dir: &str,
) -> anyhow::Result<()> {
    use std::os::windows::process::CommandExt;

    const DETACHED_PROCESS: u32 = 0x00000008;
    const CREATE_NO_WINDOW: u32 = 0x08000000;

    // 确保日志目录存在
    fs::create_dir_all(log_dir)
        .map_err(|e| anyhow::anyhow!("无法创建日志目录 {}: {}", log_dir, e))?;

    let stdout = fs::File::create(format!("{}/daemon.log", log_dir))
        .map_err(|e| anyhow::anyhow!("无法创建日志文件: {}", e))?;
    let stderr = fs::File::create(format!("{}/daemon.err", log_dir))
        .map_err(|e| anyhow::anyhow!("无法创建错误日志文件: {}", e))?;

    let exe = std::env::current_exe()?;
    let mut args = vec![
        "start".to_string(),
        "--controller-url".to_string(),
        controller_url.to_string(),
        "--token".to_string(),
        token.to_string(),
        "--log-dir".to_string(),
        log_dir.to_string(),
    ];

    if let Some(ca_path) = tls_ca_cert {
        args.push("--tls-ca-cert".to_string());
        args.push(ca_path.to_string());
    }

    let child = std::process::Command::new(&exe)
        .args(&args)
        .stdout(stdout)
        .stderr(stderr)
        .creation_flags(DETACHED_PROCESS | CREATE_NO_WINDOW)
        .spawn()
        .map_err(|e| anyhow::anyhow!("启动守护进程失败: {}", e))?;

    fs::write(pid_file, child.id().to_string())?;

    println!("守护进程已启动 (PID: {})", child.id());
    println!("PID 文件: {}", pid_file);
    println!("日志目录: {}", log_dir);
    println!();
    println!("停止守护进程: client stop --pid-file {}", pid_file);

    Ok(())
}

#[cfg(windows)]
fn stop_daemon_windows(pid_file: &str) -> anyhow::Result<()> {
    let pid_str = fs::read_to_string(pid_file)
        .map_err(|e| anyhow::anyhow!("无法读取 PID 文件 {}: {}", pid_file, e))?;
    let pid: u32 = pid_str
        .trim()
        .parse()
        .map_err(|e| anyhow::anyhow!("PID 文件内容无效: {}", e))?;

    unsafe {
        use windows_sys::Win32::Foundation::CloseHandle;
        use windows_sys::Win32::System::Threading::{OpenProcess, TerminateProcess, PROCESS_TERMINATE};

        let handle = OpenProcess(PROCESS_TERMINATE, 0, pid);
        if handle.is_null() {
            let err = std::io::Error::last_os_error();
            // ERROR_INVALID_PARAMETER (87) = process does not exist
            if err.raw_os_error() == Some(87) {
                println!("进程 (PID: {}) 已不存在", pid);
                fs::remove_file(pid_file).ok();
                return Ok(());
            }
            return Err(anyhow::anyhow!("无法打开进程 (PID: {}): {}", pid, err));
        }

        let ret = TerminateProcess(handle, 0);
        CloseHandle(handle);

        if ret == 0 {
            let err = std::io::Error::last_os_error();
            return Err(anyhow::anyhow!("停止进程失败 (PID: {}): {}", pid, err));
        }
    }

    println!("已停止守护进程 (PID: {})", pid);
    fs::remove_file(pid_file).ok();
    Ok(())
}

/// 更新二进制文件到最新版本
fn update_binary() -> anyhow::Result<()> {
    println!("正在检查更新...");

    let bin_name = "client";
    let current_version = env!("CARGO_PKG_VERSION");
    let target = self_update::get_target();

    // 获取最新 release
    let releases = self_update::backends::github::ReleaseList::configure()
        .repo_owner("oxiproxy")
        .repo_name("oxiproxy")
        .build()?
        .fetch()?;

    let latest = releases
        .first()
        .ok_or_else(|| anyhow::anyhow!("未找到任何 release"))?;

    if !self_update::version::bump_is_compatible(current_version, &latest.version)? {
        println!("最新版本 v{} 与当前版本 v{} 不兼容", latest.version, current_version);
        return Ok(());
    }

    if !self_update::version::bump_is_greater(current_version, &latest.version)? {
        println!("✓ 已是最新版本: v{}", current_version);
        return Ok(());
    }

    // 手动筛选 asset：必须同时包含组件名和 target
    let asset = latest
        .assets
        .iter()
        .find(|a| a.name.contains(bin_name) && a.name.contains(target))
        .ok_or_else(|| {
            anyhow::anyhow!(
                "未找到匹配的 release asset (bin={}, target={})",
                bin_name,
                target
            )
        })?;

    println!("当前版本: v{}", current_version);
    println!("最新版本: v{}", latest.version);
    println!("下载: {}", asset.name);

    // 下载到临时文件
    let tmp_dir = tempfile::TempDir::new()?;
    let tmp_archive_path = tmp_dir.path().join(&asset.name);
    let mut tmp_archive = std::fs::File::create(&tmp_archive_path)?;

    let mut download = self_update::Download::from_url(&asset.download_url);
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert(
        reqwest::header::ACCEPT,
        "application/octet-stream".parse().unwrap(),
    );
    download.set_headers(headers);
    download.show_progress(true);
    download.download_to(&mut tmp_archive)?;

    // 解压并替换二进制
    let bin_path_in_archive = format!("{}{}", bin_name, std::env::consts::EXE_SUFFIX);
    self_update::Extract::from_source(&tmp_archive_path)
        .extract_file(tmp_dir.path(), &bin_path_in_archive)?;

    let new_exe = tmp_dir.path().join(&bin_path_in_archive);
    self_update::self_replace::self_replace(&new_exe)?;

    println!("✓ 成功更新到版本: v{}", latest.version);
    println!("请重启 client 服务以使用新版本");

    Ok(())
}
