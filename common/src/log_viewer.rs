//! 查看本地日志文件（daemon 模式落盘的滚动日志）。
//!
//! 三端（controller/node/client）共用，仅传入不同的文件前缀
//! （`controller.log` / `node.log` / `client.log`）。

use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::Duration;

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

/// 读取文件并返回末尾 `n` 行（不含换行符）。文件不足 `n` 行则返回全部。
fn read_tail_lines(path: &Path, n: usize) -> std::io::Result<Vec<String>> {
    let content = fs::read_to_string(path)?;
    let lines: Vec<&str> = content.lines().collect();
    let start = lines.len().saturating_sub(n);
    Ok(lines[start..].iter().map(|s| s.to_string()).collect())
}

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

    let mut offset = fs::metadata(&current).map(|m| m.len()).unwrap_or(0);
    loop {
        std::thread::sleep(Duration::from_millis(500));

        if let Some(latest) = locate_latest(dir, prefix) {
            if latest != current {
                current = latest;
                offset = 0;
            }
        }

        let mut file = match fs::File::open(&current) {
            Ok(f) => f,
            Err(_) => continue,
        };
        let len = match file.metadata() {
            Ok(m) => m.len(),
            Err(_) => continue,
        };
        if len < offset {
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

    #[test]
    fn tail_returns_last_n_lines() {
        let dir = std::env::temp_dir().join(format!("oxi_log_test_tail_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let f = dir.join("x.log");
        fs::write(&f, b"l1\nl2\nl3\nl4\nl5\n").unwrap();

        assert_eq!(read_tail_lines(&f, 2).unwrap(), vec!["l4".to_string(), "l5".to_string()]);
        assert_eq!(
            read_tail_lines(&f, 10).unwrap(),
            vec!["l1", "l2", "l3", "l4", "l5"]
                .into_iter().map(String::from).collect::<Vec<_>>()
        );
        fs::remove_dir_all(&dir).unwrap();
    }
}
