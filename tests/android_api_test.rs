//! Android integration test for list_interfaces and watch_interfaces via the test app.
//! Ignored by default: requires Linux host with Android SDK + emulator + adb in PATH.

#![cfg(target_os = "linux")]

use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EventKind {
    List,
    Watch,
}

#[derive(Debug)]
struct Event {
    kind: EventKind,
    has_dot1: bool,
    has_dot10: bool,
}

#[test]
#[ignore]
fn android_list_and_watch_interfaces() {
    let _ = Command::new("adb").args(["root"]).status();

    // build and install the app
    let status = Command::new("sh")
        .current_dir("android")
        .args(["gradlew", "installDebug"])
        .status()
        .expect("failed to run sh gradlew installDebug");
    assert!(status.success(), "gradle installDebug failed");

    // Clear old logs to avoid replay confusion
    let _ = Command::new("adb").args(["logcat", "-c"]).status();
    let (rx, _handle) = spawn_logcat_watcher();

    // Start the activity
    let status = Command::new("adb")
        .args([
            "shell",
            "am",
            "start",
            "-n",
            "net.octet_stream.netwatcher.netwatchertestapp/.MainActivity",
        ])
        .status()
        .expect("failed to start activity");
    assert!(status.success(), "activity start failed");

    // Expect LIST_IPS with 127.0.0.1
    expect_event(&rx, EventKind::List, true, false, 60, "LIST_IPS initial");
    // Expect initial WATCH_IPS with 127.0.0.1 only
    expect_event(&rx, EventKind::Watch, true, false, 60, "WATCH_IPS initial");

    // Add IP 127.0.0.10
    let status = Command::new("adb")
        .args(["shell", "ip", "addr", "add", "127.0.0.10/8", "dev", "lo"])
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .expect("failed to run 'adb shell ip addr add 127.0.0.10/8 dev lo'");
    assert!(
        status.success(),
        "adb add loopback alias failed: status={status:?}"
    );
    expect_event(&rx, EventKind::Watch, true, true, 60, "WATCH_IPS after add");

    // Remove IP
    let status = Command::new("adb")
        .args(["shell", "ip", "addr", "del", "127.0.0.10/8", "dev", "lo"])
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .expect("failed to run 'adb shell ip addr del 127.0.0.10/8 dev lo'");
    assert!(
        status.success(),
        "adb delete loopback alias failed: status={status:?}"
    );
    expect_event(
        &rx,
        EventKind::Watch,
        true,
        false,
        60,
        "WATCH_IPS after del",
    );
}

fn spawn_logcat_watcher() -> (Receiver<Event>, Child) {
    let mut child = Command::new("adb")
        .args(["logcat", "-v", "brief"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to start adb logcat");
    let stdout = child.stdout.take().expect("no stdout");

    let (tx, rx) = mpsc::channel();

    thread::spawn(move || {
        let reader = BufReader::new(stdout);
        for line in reader.lines().map_while(Result::ok) {
            if let Some(ev) = parse_log_line(&line) {
                let _ = tx.send(ev);
            }
        }
    });
    (rx, child)
}

fn parse_log_line(line: &str) -> Option<Event> {
    let trimmed = line.trim();
    if let Some(idx) = trimmed.find("LIST_IPS:") {
        let list = &trimmed[idx + "LIST_IPS:".len()..];
        let ips: Vec<&str> = list
            .split(',')
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .collect();
        let has_dot1 = ips.contains(&"127.0.0.1");
        let has_dot10 = ips.contains(&"127.0.0.10");
        return Some(Event {
            kind: EventKind::List,
            has_dot1,
            has_dot10,
        });
    }
    if let Some(idx) = trimmed.find("WATCH_IPS:") {
        let list = &trimmed[idx + "WATCH_IPS:".len()..];
        let ips: Vec<&str> = list
            .split(',')
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .collect();
        let has_dot1 = ips.contains(&"127.0.0.1");
        let has_dot10 = ips.contains(&"127.0.0.10");
        return Some(Event {
            kind: EventKind::Watch,
            has_dot1,
            has_dot10,
        });
    }
    None
}

fn expect_event(
    rx: &Receiver<Event>,
    kind: EventKind,
    dot1: bool,
    dot10: bool,
    timeout_secs: u64,
    ctx: &str,
) {
    let ev = rx
        .recv_timeout(Duration::from_secs(timeout_secs))
        .unwrap_or_else(|_| panic!("timeout waiting for {ctx} {kind:?}"));
    assert_eq!(ev.kind, kind, "{ctx} kind mismatch: got {:?}", ev.kind);
    assert_eq!(ev.has_dot1, dot1, "{ctx} dot1");
    assert_eq!(ev.has_dot10, dot10, "{ctx} dot10");
}
