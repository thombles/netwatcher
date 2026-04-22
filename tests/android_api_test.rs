//! Android integration test for list_interfaces, watch_interfaces_with_callback,
//! watch_interfaces_blocking, and watch_interfaces_async via the test app.
//! Ignored by default: requires Linux host with Android SDK + emulator + adb in PATH.

#![cfg(target_os = "linux")]

use std::collections::VecDeque;
use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EventKind {
    List,
    Watch,
    BlockingWatch,
    AsyncWatch,
}

#[derive(Debug)]
struct Event {
    kind: EventKind,
    body: String,
}

#[test]
#[ignore]
fn android_list_and_watch_interfaces() {
    wait_for_adb_device(60, "connect to emulator");
    wait_for_wifi_service(120, "wait for Wi-Fi service before launch");
    set_wifi_enabled(false);
    wait_for_wifi_enabled(false, 30, "disable Wi-Fi before launch");

    // build and install the app
    wait_for_adb_device(60, "device before installDebug");
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

    let mut pending_events = VecDeque::new();

    expect_event(
        &rx,
        &mut pending_events,
        EventKind::List,
        60,
        "LIST_IPS initial without wlan0",
        |body| !body.contains("wlan0:"),
    );
    expect_event(
        &rx,
        &mut pending_events,
        EventKind::Watch,
        60,
        "WATCH_IPS initial without wlan0",
        |body| !body.contains("wlan0:"),
    );
    expect_event(
        &rx,
        &mut pending_events,
        EventKind::BlockingWatch,
        60,
        "BLOCKING_WATCH_IPS initial without wlan0",
        |body| !body.contains("wlan0:"),
    );
    expect_event(
        &rx,
        &mut pending_events,
        EventKind::AsyncWatch,
        60,
        "ASYNC_WATCH_IPS initial without wlan0",
        |body| !body.contains("wlan0:"),
    );

    set_wifi_enabled(true);
    wait_for_wifi_enabled(true, 30, "enable Wi-Fi after launch");

    expect_event(
        &rx,
        &mut pending_events,
        EventKind::Watch,
        60,
        "WATCH_IPS after enabling Wi-Fi",
        |body| body.contains("wlan0:"),
    );
    expect_event(
        &rx,
        &mut pending_events,
        EventKind::BlockingWatch,
        60,
        "BLOCKING_WATCH_IPS after enabling Wi-Fi",
        |body| body.contains("wlan0:"),
    );
    expect_event(
        &rx,
        &mut pending_events,
        EventKind::AsyncWatch,
        60,
        "ASYNC_WATCH_IPS after enabling Wi-Fi",
        |body| body.contains("wlan0:"),
    );

    set_wifi_enabled(false);
    wait_for_wifi_enabled(false, 30, "disable Wi-Fi after enabling");

    expect_event(
        &rx,
        &mut pending_events,
        EventKind::Watch,
        60,
        "WATCH_IPS after disabling Wi-Fi",
        |body| !body.contains("wlan0:"),
    );
    expect_event(
        &rx,
        &mut pending_events,
        EventKind::BlockingWatch,
        60,
        "BLOCKING_WATCH_IPS after disabling Wi-Fi",
        |body| !body.contains("wlan0:"),
    );
    expect_event(
        &rx,
        &mut pending_events,
        EventKind::AsyncWatch,
        60,
        "ASYNC_WATCH_IPS after disabling Wi-Fi",
        |body| !body.contains("wlan0:"),
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
        return Some(Event {
            kind: EventKind::List,
            body: trimmed[idx + "LIST_IPS:".len()..].trim().to_string(),
        });
    }
    if let Some(idx) = trimmed.find("ASYNC_WATCH_IPS:") {
        return Some(Event {
            kind: EventKind::AsyncWatch,
            body: trimmed[idx + "ASYNC_WATCH_IPS:".len()..].trim().to_string(),
        });
    }
    if let Some(idx) = trimmed.find("BLOCKING_WATCH_IPS:") {
        return Some(Event {
            kind: EventKind::BlockingWatch,
            body: trimmed[idx + "BLOCKING_WATCH_IPS:".len()..]
                .trim()
                .to_string(),
        });
    }
    if let Some(idx) = trimmed.find("WATCH_IPS:") {
        return Some(Event {
            kind: EventKind::Watch,
            body: trimmed[idx + "WATCH_IPS:".len()..].trim().to_string(),
        });
    }
    None
}

fn set_wifi_enabled(enabled: bool) {
    let arg = if enabled { "enabled" } else { "disabled" };
    let status = Command::new("adb")
        .args(["shell", "cmd", "wifi", "set-wifi-enabled", arg])
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .expect("failed to run 'adb shell cmd wifi set-wifi-enabled'");
    assert!(
        status.success(),
        "failed to set Wi-Fi {arg}: status={status:?}"
    );
}

fn wait_for_adb_device(timeout_secs: u64, ctx: &str) {
    let deadline = Instant::now() + Duration::from_secs(timeout_secs);

    loop {
        let output = Command::new("adb")
            .args(["get-state"])
            .output()
            .expect("failed to run 'adb get-state'");
        if output.status.success() && String::from_utf8_lossy(&output.stdout).trim() == "device" {
            return;
        }
        if Instant::now() >= deadline {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            panic!("timeout waiting for adb device to {ctx}: stdout={stdout:?} stderr={stderr:?}");
        }
        thread::sleep(Duration::from_millis(500));
    }
}

fn wait_for_wifi_service(timeout_secs: u64, ctx: &str) {
    let deadline = Instant::now() + Duration::from_secs(timeout_secs);

    loop {
        let service_output = Command::new("adb")
            .args(["shell", "service", "check", "wifi"])
            .output()
            .expect("failed to run 'adb shell service check wifi'");
        let service_stdout = String::from_utf8_lossy(&service_output.stdout);

        let dumpsys_output = Command::new("adb")
            .args(["shell", "dumpsys", "wifi"])
            .output()
            .expect("failed to run 'adb shell dumpsys wifi'");
        let dumpsys_stdout = String::from_utf8_lossy(&dumpsys_output.stdout);

        if service_output.status.success()
            && service_stdout.contains("Service wifi: found")
            && dumpsys_output.status.success()
            && !dumpsys_stdout.contains("Can't find service: wifi")
        {
            return;
        }

        if Instant::now() >= deadline {
            let service_stderr = String::from_utf8_lossy(&service_output.stderr);
            let dumpsys_stderr = String::from_utf8_lossy(&dumpsys_output.stderr);
            panic!(
                "timeout waiting to {ctx}: service_stdout={service_stdout:?} service_stderr={service_stderr:?} dumpsys_stdout={dumpsys_stdout:?} dumpsys_stderr={dumpsys_stderr:?}"
            );
        }

        thread::sleep(Duration::from_millis(500));
    }
}

fn wait_for_wifi_enabled(enabled: bool, timeout_secs: u64, ctx: &str) {
    let expected = if enabled {
        "Wifi is enabled"
    } else {
        "Wifi is disabled"
    };
    let deadline = Instant::now() + Duration::from_secs(timeout_secs);

    loop {
        let output = Command::new("adb")
            .args(["shell", "cmd", "wifi", "status"])
            .output()
            .expect("failed to run 'adb shell cmd wifi status'");
        let stdout = String::from_utf8_lossy(&output.stdout);
        if stdout.contains(expected) {
            return;
        }
        if Instant::now() >= deadline {
            panic!("timeout waiting to {ctx}: got {stdout:?}");
        }
        thread::sleep(Duration::from_millis(500));
    }
}

fn expect_event(
    rx: &Receiver<Event>,
    pending_events: &mut VecDeque<Event>,
    kind: EventKind,
    timeout_secs: u64,
    ctx: &str,
    predicate: impl Fn(&str) -> bool,
) {
    let deadline = Instant::now() + Duration::from_secs(timeout_secs);
    let mut last_body = None;

    loop {
        let mut remaining_events = VecDeque::new();
        while let Some(event) = pending_events.pop_front() {
            if event.kind != kind {
                remaining_events.push_back(event);
                continue;
            }
            if predicate(&event.body) {
                *pending_events = remaining_events;
                return;
            }
            last_body = Some(event.body);
        }
        *pending_events = remaining_events;

        let now = Instant::now();
        if now >= deadline {
            match last_body {
                Some(body) => {
                    panic!("timeout waiting for {ctx} {kind:?}; last matching body: {body}")
                }
                None => panic!("timeout waiting for {ctx} {kind:?}"),
            }
        }

        let ev = rx
            .recv_timeout(deadline.saturating_duration_since(now))
            .unwrap_or_else(|_| panic!("timeout waiting for {ctx} {kind:?}"));
        if ev.kind != kind {
            pending_events.push_back(ev);
            continue;
        }
        if predicate(&ev.body) {
            return;
        }
        last_body = Some(ev.body);
    }
}
