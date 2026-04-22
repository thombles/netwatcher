#[cfg(all(
    any(target_os = "windows", target_os = "linux", target_vendor = "apple"),
    any(feature = "tokio", feature = "async-io")
))]
use netwatcher::IpRecord;
#[cfg(all(
    any(target_os = "windows", target_os = "linux", target_vendor = "apple"),
    any(feature = "tokio", feature = "async-io")
))]
use serial_test::serial;
#[cfg(all(
    any(target_os = "windows", target_os = "linux", target_vendor = "apple"),
    any(feature = "tokio", feature = "async-io")
))]
use std::net::{IpAddr, Ipv4Addr};

#[cfg(all(
    any(target_os = "windows", target_os = "linux", target_vendor = "apple"),
    any(feature = "tokio", feature = "async-io")
))]
mod helpers;

#[cfg(all(
    any(target_os = "windows", target_os = "linux", target_vendor = "apple"),
    any(feature = "tokio", feature = "async-io")
))]
fn loopback_expectations() -> (String, IpRecord, IpRecord) {
    use helpers::sys::*;

    let loopback_interface = discover_loopback_interface();
    println!("discovered loopback interface: '{loopback_interface}'");

    let expected_original = IpRecord {
        ip: IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)),
        prefix_len: 8,
    };
    let expected_added = IpRecord {
        ip: IpAddr::V4(Ipv4Addr::new(127, 0, 0, 10)),
        prefix_len: 8,
    };

    (loopback_interface, expected_original, expected_added)
}

#[cfg(all(
    any(target_os = "windows", target_os = "linux", target_vendor = "apple"),
    any(feature = "tokio", feature = "async-io")
))]
async fn run_async_watch_scenario(mut watch: netwatcher::AsyncWatch) {
    use helpers::assert_update_has_ip;
    use helpers::sys::*;

    let (loopback_interface, expected_original, expected_added) = loopback_expectations();

    let initial = watch.changed().await;
    assert!(initial.is_initial);
    assert_update_has_ip(&initial, &expected_original, true);
    assert_update_has_ip(&initial, &expected_added, false);

    add_ip_to_interface(&loopback_interface, "127.0.0.10");
    let added = watch.changed().await;
    assert!(!added.is_initial);
    assert_update_has_ip(&added, &expected_original, true);
    assert_update_has_ip(&added, &expected_added, true);

    remove_ip_from_interface(&loopback_interface, "127.0.0.10");
    let removed = watch.changed().await;
    assert!(!removed.is_initial);
    assert_update_has_ip(&removed, &expected_original, true);
    assert_update_has_ip(&removed, &expected_added, false);
}

#[test]
#[ignore]
#[cfg(all(
    any(target_os = "windows", target_os = "linux", target_vendor = "apple"),
    feature = "tokio"
))]
#[serial(loopback)]
fn test_watch_interfaces_async_tokio_loopback_changes() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("failed to build tokio runtime");
    runtime.block_on(async {
        let watch = netwatcher::watch_interfaces_async::<netwatcher::async_adapter::Tokio>()
            .expect("failed to create async watcher");
        run_async_watch_scenario(watch).await;
    });
}

#[test]
#[ignore]
#[cfg(all(
    any(target_os = "windows", target_os = "linux", target_vendor = "apple"),
    feature = "async-io"
))]
#[serial(loopback)]
fn test_watch_interfaces_async_async_io_loopback_changes() {
    async_io::block_on(async {
        let watch = netwatcher::watch_interfaces_async::<netwatcher::async_adapter::AsyncIo>()
            .expect("failed to create async watcher");
        run_async_watch_scenario(watch).await;
    });
}
