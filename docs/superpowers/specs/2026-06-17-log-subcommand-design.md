# `log` 子命令设计 — 查看本地日志

日期：2026-06-17

## 目标

为 `controller`、`node`、`client` 三个二进制各新增一个 `log` 子命令，用于查看**自己**在 daemon 模式下落盘的日志：

- `controller log` → 查看 controller 日志
- `node log` → 查看 node 日志
- `client log` → 查看 client 日志

## 背景

三个程序在 daemon 模式下都通过 `tracing_appender::rolling::daily(log_dir, prefix)` 将日志按天分割写入 `--log-dir`（默认 `./logs`）：

- controller → `logs/controller.log.YYYY-MM-DD`（`controller/src/main.rs:645`）
- node → `logs/node.log.YYYY-MM-DD`（`node/src/server/mod.rs:41`）
- client → `logs/client.log.YYYY-MM-DD`（`client/src/client/mod.rs:27`）

前台 `start` 模式日志只输出到终端（无文件）。内存日志缓冲区（`node_logs.rs` 等）属于运行中进程私有，独立的 CLI 进程无法访问，因此 `log` 命令只能读磁盘文件。

## 命令参数（三端一致）

| 参数 | 默认 | 说明 |
|------|------|------|
| `--log-dir <DIR>` | `./logs` | 日志目录，与 daemon 默认一致 |
| `-n, --lines <N>` | `200` | 打印末尾行数 |
| `-f, --follow` | false | 实时跟随（类似 `tail -f`），Ctrl-C 退出 |

`--help` 中需说明：仅查看 daemon 模式落盘的日志；前台 `start` 模式日志只在终端。

## 架构

### 共享模块 `common/src/log_viewer.rs`

仿照现有 `common/src/process_status.rs` 的复用方式，三端调用同一函数，仅传入不同的文件前缀。

公开接口（签名示意，最终以实现为准）：

```rust
/// 查看日志：定位 log_dir 中匹配 `{prefix}.*` 的最新按天文件，打印末尾 lines 行；
/// follow=true 时持续跟随新追加内容，跨天滚动自动切换到新文件，直到进程被终止。
/// 返回进程退出码（0 成功 / 非 0 失败，便于 main 中 std::process::exit）。
pub fn run(log_dir: &str, prefix: &str, lines: usize, follow: bool) -> i32;
```

prefix 取值：`"controller.log"` / `"node.log"` / `"client.log"`。

### 逻辑流程

1. **定位文件**
   - 列出 `log_dir` 下文件名以 `{prefix}.` 开头的项，按文件名降序排序，取第一个（日期后缀 `YYYY-MM-DD` 字典序即时间序）。
   - 若无带日期文件，回退到 `{prefix}` 本身（兼容非滚动写法）。
   - 都不存在：打印友好提示「未找到日志文件，可能未以 daemon 模式运行，或日志目录不对（前台 start 模式日志只输出到终端）」，返回非 0。

2. **打印末尾 N 行**
   - 纯 Rust 读取整个文件，按行切分后取末尾 N 行打印（不依赖系统 `tail`，跨平台）。
   - 按天分割使单文件体量可控；暂不做反向块读优化（YAGNI），如后续单文件过大再优化。

3. **follow 模式**（`-f`）
   - 打印完末尾后，记录当前文件路径与字节偏移（文件末尾）。
   - 循环：sleep ~500ms → 重新解析「最新文件」：
     - 若仍是同一文件：从上次偏移读取新追加字节并打印，更新偏移。
     - 若出现更新的日期文件（跨天滚动）：切换到新文件，从偏移 0 开始读取并打印，更新当前文件与偏移。
   - 不安装额外信号处理，Ctrl-C 直接终止进程（与 `tail -f` 行为一致）。

### 各 main.rs 集成

在 `controller`、`node`、`client` 各自的 `Command` enum 增加变体：

```rust
/// 查看本地日志（daemon 模式落盘的日志）
Log {
    #[arg(long, default_value = "./logs")]
    log_dir: String,
    #[arg(short = 'n', long, default_value_t = 200)]
    lines: usize,
    #[arg(short = 'f', long)]
    follow: bool,
},
```

match 分支：

```rust
Command::Log { log_dir, lines, follow } => {
    let code = common::log_viewer::run(&log_dir, "<prefix>", lines, follow);
    std::process::exit(code);
}
```

每个二进制传入各自的 prefix。无平台特定差异（文件名三端在所有 OS 一致），`#[cfg]` 无需区分平台。

## 错误处理

- 日志目录不存在 / 读取失败：打印错误信息，返回非 0 退出码。
- follow 模式下文件读取瞬时失败（如滚动切换间隙）：不退出，下一轮重试。

## 测试

- `common/src/log_viewer.rs` 单元测试：
  - 在临时目录写多个 `{prefix}.YYYY-MM-DD` 文件，验证选中最新一个。
  - 验证末尾 N 行截取正确（文件行数 < N、= N、> N 三种）。
  - 回退到无日期 `{prefix}` 文件。
  - 目录为空 / 不存在时返回非 0。
- follow 模式的实时行为以手动验证为主（自动测试无限循环不易，至少覆盖一轮追加读取逻辑可拆成可测函数）。

## 影响范围

新增：`common/src/log_viewer.rs`、`common/src/lib.rs` 导出。
修改：`controller/src/main.rs`、`node/src/main.rs`、`client/src/main.rs`（各加一个子命令 + 分支）。
文档：`CLAUDE.md` 中三端命令行说明补充 `log` 子命令（与现有 `status` 一致）。

## 非目标（YAGNI）

- 不读取内存日志缓冲区（跨进程不可达）。
- 不做按级别/关键字过滤（用 `grep` 管道即可）。
- 不做反向块读大文件优化。
