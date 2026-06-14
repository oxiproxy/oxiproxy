# 为 Controller / Node / Client 新增 `status` 命令

日期：2026-06-14

## 目标

为三个组件（controller、node、client）各新增一个 `status` 子命令，用于查询守护进程是否在运行，并展示运行细节（PID、运行时长、内存占用）。同时在 Linux 上额外探测 systemd 服务状态，使得用 `install` 安装的服务也能被查到。

## 检查来源

`status` 综合两类来源判断运行状态：

1. **PID 文件**（所有平台）：读取 `--pid-file` 指向的文件，解析 PID，用 `kill(pid, 0)`（Unix）/ `OpenProcess`（Windows）探测进程是否存活。
2. **systemd**（仅 Linux）：调用 `systemctl is-active <service>` 与 `systemctl is-enabled <service>`，展示服务的 active/enabled 状态。这覆盖了 `install` 子命令注册的开机自启服务。

两者独立展示——一台机器可能用 daemon 模式跑（有 PID 文件），也可能用 systemd 跑（无 PID 文件）。`status` 把两类信息都打印出来，不强求一致。

## CLI 参数

每个组件新增 `Status` 变体：

- `--pid-file <PATH>`：默认值复用各自现有约定
  - controller: `/var/run/oxiproxy-controller.pid`（Windows: `oxiproxy-controller.pid`）
  - node: `/var/run/oxiproxy-node.pid`（Windows: `oxiproxy-node.pid`）
  - client: `/var/run/oxiproxy-client.pid`（Windows: `oxiproxy-client.pid`）
- `--service-name <NAME>`（仅 `#[cfg(target_os = "linux")]`）：默认值复用 install 约定
  - `oxiproxy-controller` / `oxiproxy-node` / `oxiproxy-client`

## 输出

中文文本输出，分两段：

```
守护进程 (PID 文件):
  PID 文件:   /var/run/oxiproxy-node.pid
  状态:       运行中 (PID: 12345)
  运行时长:   3天 4小时12分钟
  内存占用:   45.2 MB

systemd 服务 (oxiproxy-node):
  active:     active (running)
  enabled:    enabled
```

各种情形：

- PID 文件不存在 → `状态: 未运行 (无 PID 文件)`，跳过运行时长/内存。
- PID 文件存在但进程已死 → `状态: 已停止 (PID 文件残留: 12345)`。
- 进程存活 → 显示 PID、运行时长、内存。
- systemd 段仅 Linux 输出；`systemctl` 不可用时显示 `systemctl 不可用，跳过`。

退出码：进程存活或 systemd active → `0`；否则 `1`（方便脚本判断）。

## 运行细节获取（零新增依赖）

- **Linux**：
  - 运行时长：读 `/proc/<pid>/stat` 第 22 字段 `starttime`（单位 clock ticks），结合 `/proc/uptime` 计算进程已运行秒数。`clock ticks/秒` 用 `libc::sysconf(_SC_CLK_TCK)` 获取。
  - 内存：读 `/proc/<pid>/status` 的 `VmRSS:` 行（单位 kB）。
- **Windows**：不读细节，仅显示 PID + 存活/已停止。探活用 `OpenProcess` + `GetExitCodeProcess`，或简单复用现有 stop 路径里已有的 Windows 进程探测方式（若无则用 `tasklist` 查 PID 是否存在）。

不引入 `sysinfo` 等新 crate。`libc` 已是依赖。

## 代码组织

把可复用逻辑放进 `common`，避免三份拷贝：

- 新增 `common/src/process_status.rs`（无 cfg 限制的跨平台模块）：
  - `pub struct ProcessStatus { pub pid: i32, pub alive: bool, pub uptime_secs: Option<u64>, pub rss_bytes: Option<u64> }`
  - `pub fn probe_pid_file(pid_file: &str) -> PidFileStatus`，返回枚举：`NoFile` / `Stale(pid)` / `Running(ProcessStatus)`。
  - Linux 下实现 `/proc` 读取；Windows 下 `uptime_secs`/`rss_bytes` 返回 `None`，仅探活。
- 新增 `common/src/systemd.rs` 中的查询函数（该文件已 `#![cfg(target_os = "linux")]`）：
  - `pub fn query_service(service_name: &str) -> SystemdStatus`，内部跑 `systemctl is-active` / `is-enabled`，返回字符串状态；`systemctl` 不可用时返回标记值。
- 三个组件的 `main.rs` 各加一个 `print_status(pid_file, service_name)` 薄封装，调用上述 common 函数并按上面的格式打印 + 设置退出码。

## 测试

- `common/src/process_status.rs`：
  - 解析 `/proc/<pid>/stat` 字段的纯函数（传入构造的样例字符串）单测——注意 stat 第 2 字段 comm 可能含空格和括号，需从最后一个 `)` 之后再按空格切分。
  - 解析 `VmRSS:` 行的纯函数单测。
- `common/src/systemd.rs`：`query_service` 依赖外部 `systemctl`，不做单测，仅保证编译。
- 三个组件手动验证：`cargo run -p node -- status`（无 PID 文件场景）。

## 非目标（YAGNI）

- 不做 JSON 输出（如以后脚本需要再加 `--json`）。
- 不做远程 status（Controller 已有 Web API 查 node/client 在线状态）。
- Windows 不显示运行时长/内存。
