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

/// 读取文件并返回末尾 `n` 行（不含换行符）。文件不足 `n` 行则返回全部。
fn read_tail_lines(path: &Path, n: usize) -> std::io::Result<Vec<String>> {
    let content = fs::read_to_string(path)?;
    let lines: Vec<&str> = content.lines().collect();
    let start = lines.len().saturating_sub(n);
    Ok(lines[start..].iter().map(|s| s.to_string()).collect())
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
