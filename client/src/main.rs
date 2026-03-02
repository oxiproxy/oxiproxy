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
        /// Controller 地址（例如 http://controller:3100）
        #[arg(long)]
        controller_url: String,

        /// 客户端 Token
        #[arg(long)]
        token: String,

        /// 自定义 CA 证书文件路径（PEM 格式，用于验证 Controller 的 TLS 证书）
        #[arg(long)]
        tls_ca_cert: Option<String>,

        /// 日志目录路径（按天自动分割，不指定则输出到控制台）
        #[arg(long)]
        log_dir: Option<String>,
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
        } => {
            let ca_cert = load_tls_ca_cert(&tls_ca_cert)?;
            if let Some(ref dir) = log_dir {
                fs::create_dir_all(dir).expect("无法创建日志目录");
            }
            let runtime = tokio::runtime::Runtime::new()?;
            runtime.block_on(client::run_client(controller_url, token, ca_cert, log_dir))?;
        }

        Command::Stop { pid_file } => {
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
    }

    Ok(())
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
