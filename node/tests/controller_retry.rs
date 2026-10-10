#![cfg(target_os = "linux")]

use std::{fs, process::Stdio, time::Duration};

#[tokio::test]
async fn an_unavailable_controller_at_startup_does_not_exit_the_node() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    drop(listener);

    let temp = tempfile::tempdir().unwrap();
    let log = temp.path().join("node-output");
    let mut child = tokio::process::Command::new(env!("CARGO_BIN_EXE_node"))
        .args(["run", "--controller-url", &url, "--token", "test-token"])
        .stdout(Stdio::from(fs::File::create(&log).unwrap()))
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .unwrap();

    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            assert!(
                child.try_wait().unwrap().is_none(),
                "node exited before retrying"
            );
            let output = fs::read_to_string(&log).unwrap();
            if output.matches("首次连接 Controller 失败").count() >= 2 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("node did not retry startup connection");
    child.kill().await.unwrap();
}
