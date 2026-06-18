# `log` 子命令 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 为 controller / node / client 三个二进制各新增 `log` 子命令，查看自己 daemon 模式落盘的日志（末尾 N 行 + `-f` 跟随）。

**Architecture:** 共享逻辑放 `common/src/log_viewer.rs`，三端只传不同文件前缀（`controller.log`/`node.log`/`client.log`）。函数定位 log-dir 中最新按天文件、打印末尾 N 行，follow 模式轮询追加内容并跨天自动切换。纯 Rust 跨平台。

**Tech Stack:** Rust 2021, clap (derive), std::fs。无新增依赖。

参考 spec：`docs/superpowers/specs/2026-06-17-log-subcommand-design.md`

---

## File Structure

- **Create** `common/src/log_viewer.rs` — 共享日志查看逻辑（定位文件、读末尾行、follow 轮询）+ 单元测试
- **Modify** `common/src/lib.rs` — 注册 `pub mod log_viewer;`
- **Modify** `controller/src/main.rs` — 加 `Log` 子命令变体 + 各 match 块分支（prefix `"controller.log"`）
- **Modify** `node/src/main.rs` — 加 `Log` 子命令变体 + 各 match 块分支（prefix `"node.log"`）
- **Modify** `client/src/main.rs` — 加 `Log` 子命令变体 + 各 match 块分支（prefix `"client.log"`）
- **Modify** `CLAUDE.md` — 三端命令说明补充 `log`

---

## Task 1: 共享模块核心 — 定位最新日志文件

**Files:**
- Create: `common/src/log_viewer.rs`
- Modify: `common/src/lib.rs`

- [ ] **Step 1: 注册模块**

在 `common/src/lib.rs` 的 `pub mod process_status;` 一行下面新增一行：

```rust
pub mod log_viewer;
```

- [ ] **Step 2: 写失败测试**

创建 `common/src/log_viewer.rs`，内容如下（仅本任务部分，后续任务追加）：

```rust
//! 查看本地日志文件（daemon 模式落盘的滚动日志）。
//!
//! 三端（controller/node/client）共用，仅传入不同的文件前缀
//! （`controller.log` / `node.log` / `client.log`）。

use std::fs;
use std::path::{Path, PathBuf};

/// 在 `log_dir` 中定位匹配 `{prefix}.*` 的最新按天日志文件。
///
/// 选择规则：文件名以 `{prefix}.` 开头的项按文件名降序取第一个
/// （日期后缀 `YYYY-MM-DD` 字典序即时间序）。若没有带日期的文件，
/// 回退到与 `{prefix}` 同名的文件。都不存在则返回 `None`。
fn locate_latest(log_dir: &Path, prefix: &str) -> Option<PathBuf> {
    let dot_prefix = format!("{}.", prefix);
    let mut dated: Vec<PathBuf> = Vec::new();
    if let Ok(entries) = fs::read_dir(log_dir) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.starts_with(&dot_prefix) {
                dated.push(entry.path());
            }
        }
    }
    if !dated.is_empty() {
        dated.sort();
        return dated.pop(); // 降序最大 = 排序后最后一个
    }
    let fallback = log_dir.join(prefix);
    if fallback.is_file() {
        Some(fallback)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_latest_dated_file() {
        let dir = std::env::temp_dir().join(format!("oxi_log_test_latest_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("node.log.2026-06-15"), b"old\n").unwrap();
        fs::write(dir.join("node.log.2026-06-17"), b"new\n").unwrap();
        fs::write(dir.join("node.log.2026-06-16"), b"mid\n").unwrap();

        let got = locate_latest(&dir, "node.log").unwrap();
        assert_eq!(got.file_name().unwrap().to_string_lossy(), "node.log.2026-06-17");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn falls_back_to_undated_file() {
        let dir = std::env::temp_dir().join(format!("oxi_log_test_fallback_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("client.log"), b"data\n").unwrap();

        let got = locate_latest(&dir, "client.log").unwrap();
        assert_eq!(got.file_name().unwrap().to_string_lossy(), "client.log");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn returns_none_when_empty() {
        let dir = std::env::temp_dir().join(format!("oxi_log_test_none_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        assert!(locate_latest(&dir, "controller.log").is_none());
        fs::remove_dir_all(&dir).unwrap();
    }
}
```

- [ ] **Step 3: 运行测试确认通过**

Run: `cargo test -p common log_viewer -- --nocapture`
Expected: 3 个 `locate_latest` 相关测试 PASS（实现已随测试一起给出）。

- [ ] **Step 4: 提交**

```bash
git add common/src/lib.rs common/src/log_viewer.rs
git commit -m "feat(common): log_viewer 定位最新按天日志文件"
```

---

## Task 2: 读取末尾 N 行

**Files:**
- Modify: `common/src/log_viewer.rs`

- [ ] **Step 1: 写失败测试**

在 `common/src/log_viewer.rs` 的 `#[cfg(test)] mod tests` 内追加测试：

```rust
    #[test]
    fn tail_returns_last_n_lines() {
        let dir = std::env::temp_dir().join(format!("oxi_log_test_tail_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let f = dir.join("x.log");
        fs::write(&f, b"l1\nl2\nl3\nl4\nl5\n").unwrap();

        // 文件行数 > N
        assert_eq!(read_tail_lines(&f, 2).unwrap(), vec!["l4".to_string(), "l5".to_string()]);
        // N >= 文件行数
        assert_eq!(
            read_tail_lines(&f, 10).unwrap(),
            vec!["l1", "l2", "l3", "l4", "l5"]
                .into_iter().map(String::from).collect::<Vec<_>>()
        );
        fs::remove_dir_all(&dir).unwrap();
    }
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test -p common log_viewer::tests::tail_returns_last_n_lines`
Expected: 编译失败 `cannot find function read_tail_lines`。

- [ ] **Step 3: 实现 `read_tail_lines`**

在 `common/src/log_viewer.rs` 的 `locate_latest` 函数下方新增：

```rust
/// 读取文件并返回末尾 `n` 行（不含换行符）。文件不足 `n` 行则返回全部。
fn read_tail_lines(path: &Path, n: usize) -> std::io::Result<Vec<String>> {
    let content = fs::read_to_string(path)?;
    let lines: Vec<&str> = content.lines().collect();
    let start = lines.len().saturating_sub(n);
    Ok(lines[start..].iter().map(|s| s.to_string()).collect())
}
```

- [ ] **Step 4: 运行测试确认通过**

Run: `cargo test -p common log_viewer`
Expected: 全部 PASS。

- [ ] **Step 5: 提交**

```bash
git add common/src/log_viewer.rs
git commit -m "feat(common): log_viewer 读取末尾 N 行"
```

---

## Task 3: 公开入口 `run`（含 follow 模式）

**Files:**
- Modify: `common/src/log_viewer.rs`

- [ ] **Step 1: 实现 `run`**

在 `common/src/log_viewer.rs` 顶部 `use` 区补充：

```rust
use std::io::{Read, Seek, SeekFrom};
use std::time::Duration;
```

在文件末尾（`#[cfg(test)]` 之前）新增公开函数：

```rust
/// 查看日志：定位 `log_dir` 中匹配 `{prefix}.*` 的最新按天文件，
/// 打印末尾 `lines` 行；`follow=true` 时持续跟随新追加内容，
/// 跨天滚动自动切换到新文件，直到进程被终止（Ctrl-C）。
///
/// 返回进程退出码：0 成功；非 0 表示未找到日志或读取失败。
pub fn run(log_dir: &str, prefix: &str, lines: usize, follow: bool) -> i32 {
    let dir = Path::new(log_dir);

    let mut current = match locate_latest(dir, prefix) {
        Some(p) => p,
        None => {
            eprintln!(
                "未找到日志文件（前缀 {}）于目录 {}。\n\
                 可能未以 daemon 模式运行，或日志目录不对（前台 start 模式日志只输出到终端）。",
                prefix, log_dir
            );
            return 1;
        }
    };

    // 打印末尾 N 行
    match read_tail_lines(&current, lines) {
        Ok(tail) => {
            for line in &tail {
                println!("{}", line);
            }
        }
        Err(e) => {
            eprintln!("读取日志失败 {}: {}", current.display(), e);
            return 1;
        }
    }

    if !follow {
        return 0;
    }

    // follow：记录当前文件末尾偏移，循环读取追加内容
    let mut offset = fs::metadata(&current).map(|m| m.len()).unwrap_or(0);
    loop {
        std::thread::sleep(Duration::from_millis(500));

        // 检测跨天滚动：出现更新的日期文件则切换，从头读取
        if let Some(latest) = locate_latest(dir, prefix) {
            if latest != current {
                current = latest;
                offset = 0;
            }
        }

        let mut file = match fs::File::open(&current) {
            Ok(f) => f,
            Err(_) => continue, // 滚动切换瞬时失败，下轮重试
        };
        let len = match file.metadata() {
            Ok(m) => m.len(),
            Err(_) => continue,
        };
        if len < offset {
            // 文件被截断/替换，从头读
            offset = 0;
        }
        if len > offset {
            if file.seek(SeekFrom::Start(offset)).is_err() {
                continue;
            }
            let mut buf = String::new();
            if file.read_to_string(&mut buf).is_ok() {
                print!("{}", buf);
                offset = len;
            }
        }
    }
}
```

- [ ] **Step 2: 验证编译 + 现有测试不回归**

Run: `cargo test -p common log_viewer`
Expected: 编译通过，已有测试全部 PASS（`run` 的 follow 行为靠手动验证，不写无限循环测试）。

- [ ] **Step 3: 提交**

```bash
git add common/src/log_viewer.rs
git commit -m "feat(common): log_viewer run 入口与 follow 跟随模式"
```

---

## Task 4: controller 接入 `log` 子命令

**Files:**
- Modify: `controller/src/main.rs`

> 说明：controller `main.rs` 有多个 cfg 分流的执行路径，`Command::Status` 在约 `:213`/`:222`/`:417` 出现多处。`Log` 变体需在 enum 定义一次，并在**每一个**处理 `Command::*` 的 match 块里加分支（与 `Status` 同样的位置）。`Log` 无平台特定参数，无需 `#[cfg]`。

- [ ] **Step 1: 在 `Command` enum 增加变体**

在 `controller/src/main.rs` 的 `Command` enum 中，`Status { ... }` 变体之后新增：

```rust
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
```

- [ ] **Step 2: 在每个 match 块加分支**

在 `controller/src/main.rs` 中，找到每一处 `Command::Status { .. } => { ... }` 分支，在其**相邻位置**加入（每个 match 块都要加一次）：

```rust
        Command::Log { log_dir, lines, follow } => {
            let code = common::log_viewer::run(&log_dir, "controller.log", lines, follow);
            std::process::exit(code);
        }
```

定位命令辅助：`grep -n "Command::Status" controller/src/main.rs`，每个出现 `Status` 的 match 块都补一个 `Log` 分支。

- [ ] **Step 3: 编译验证**

Run: `cargo build -p controller`
Expected: 编译通过，无 `non-exhaustive patterns` 错误（若报缺分支，说明某个 match 块漏加）。

- [ ] **Step 4: 手动冒烟测试**

Run:
```bash
mkdir -p /tmp/oxitest && printf 'a\nb\nc\n' > /tmp/oxitest/controller.log.2026-06-17
cargo run -p controller -- log --log-dir /tmp/oxitest -n 2
```
Expected: 输出
```
b
c
```

- [ ] **Step 5: 提交**

```bash
git add controller/src/main.rs
git commit -m "feat(controller): 新增 log 子命令查看本地日志"
```

---

## Task 5: node 接入 `log` 子命令

**Files:**
- Modify: `node/src/main.rs`

> node `main.rs` 同样有多个 cfg 分流执行路径（`Command::Status` 在约 `:282`/`:291`/`:534` 等处）。做法同 Task 4，prefix 改为 `"node.log"`。

- [ ] **Step 1: 在 `Command` enum 增加变体**

在 `node/src/main.rs` 的 `Command` enum 中，`Status { ... }` 变体之后新增（内容与 controller 完全一致）：

```rust
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
```

- [ ] **Step 2: 在每个 match 块加分支**

`grep -n "Command::Status" node/src/main.rs`，在每个出现 `Status` 的 match 块加入：

```rust
        Command::Log { log_dir, lines, follow } => {
            let code = common::log_viewer::run(&log_dir, "node.log", lines, follow);
            std::process::exit(code);
        }
```

- [ ] **Step 3: 编译验证**

Run: `cargo build -p node`
Expected: 编译通过。

- [ ] **Step 4: 手动冒烟测试**

Run:
```bash
printf 'n1\nn2\nn3\n' > /tmp/oxitest/node.log.2026-06-17
cargo run -p node -- log --log-dir /tmp/oxitest -n 2
```
Expected: 输出
```
n2
n3
```

- [ ] **Step 5: 提交**

```bash
git add node/src/main.rs
git commit -m "feat(node): 新增 log 子命令查看本地日志"
```

---

## Task 6: client 接入 `log` 子命令

**Files:**
- Modify: `client/src/main.rs`

> client `main.rs` 同样有多个 cfg 分流执行路径（`Command::Status` 在约 `:299`/`:308`/`:534` 等处）。做法同 Task 4，prefix 改为 `"client.log"`。

- [ ] **Step 1: 在 `Command` enum 增加变体**

在 `client/src/main.rs` 的 `Command` enum 中，`Status { ... }` 变体之后新增（内容与 controller 完全一致）：

```rust
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
```

- [ ] **Step 2: 在每个 match 块加分支**

`grep -n "Command::Status" client/src/main.rs`，在每个出现 `Status` 的 match 块加入：

```rust
        Command::Log { log_dir, lines, follow } => {
            let code = common::log_viewer::run(&log_dir, "client.log", lines, follow);
            std::process::exit(code);
        }
```

- [ ] **Step 3: 编译验证**

Run: `cargo build -p client`
Expected: 编译通过。

- [ ] **Step 4: 手动冒烟测试**

Run:
```bash
printf 'c1\nc2\nc3\n' > /tmp/oxitest/client.log.2026-06-17
cargo run -p client -- log --log-dir /tmp/oxitest -n 2
```
Expected: 输出
```
c2
c3
```

- [ ] **Step 5: 提交**

```bash
git add client/src/main.rs
git commit -m "feat(client): 新增 log 子命令查看本地日志"
```

---

## Task 7: 文档与最终校验

**Files:**
- Modify: `CLAUDE.md`

- [ ] **Step 1: 更新 CLAUDE.md**

在 `CLAUDE.md` 「后端开发」命令区块（`cargo run ... -p node`、`-p client` 附近）补充 `log` 子命令示例：

```bash
# 查看本地日志（daemon 模式落盘的日志，末尾 200 行）
cargo run --release -p controller -- log
cargo run --release -p node -- log -n 500
cargo run --release -p client -- log -f   # 实时跟随
```

并在 Node/Client 模块说明中，`main.rs` 的 CLI 子命令列表里把 `log` 加入（与 `status` 并列）。

- [ ] **Step 2: 全量校验**

Run:
```bash
cargo test -p common log_viewer
cargo clippy --all-targets --all-features -- -D warnings
cargo build --release
```
Expected: 测试 PASS、clippy 无警告、release 构建成功。

- [ ] **Step 3: follow 模式人工验证**

Run（两个终端）：
```bash
# 终端 A
cargo run -p node -- log --log-dir /tmp/oxitest -f
# 终端 B
echo "new line appended" >> /tmp/oxitest/node.log.2026-06-17
```
Expected: 终端 A 在 ~0.5s 内打印出 `new line appended`；Ctrl-C 能正常退出。完成后 `rm -rf /tmp/oxitest`。

- [ ] **Step 4: 提交**

```bash
git add CLAUDE.md
git commit -m "docs: 补充 log 子命令到组件说明"
```

---

## 自检结论

- **Spec 覆盖**：定位最新文件(Task1)、末尾 N 行(Task2)、follow+入口(Task3)、三端接入(Task4-6)、文档(Task7) 全覆盖。
- **类型一致**：`run(log_dir: &str, prefix: &str, lines: usize, follow: bool) -> i32` 在三端调用签名一致；`locate_latest`/`read_tail_lines` 私有辅助签名前后一致。
- **无占位符**：所有代码步骤含完整代码。
- **注意点**：每个二进制 `main.rs` 有多个 cfg 分流的 match 块，`Log` 分支需在每处都加（编译器 non-exhaustive 报错可兜底发现）。
