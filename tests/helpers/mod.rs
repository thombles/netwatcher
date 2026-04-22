use netwatcher::{IpRecord, Update};

#[cfg(target_os = "windows")]
#[path = "sys_windows.rs"]
pub mod sys;

#[cfg(target_os = "linux")]
#[path = "sys_linux.rs"]
pub mod sys;

#[cfg(target_vendor = "apple")]
#[path = "sys_mac.rs"]
pub mod sys;

pub fn assert_update_has_ip(update: &Update, ip_record: &IpRecord, should_have: bool) {
    let has_ip = update
        .interfaces
        .values()
        .any(|interface| interface.ips.contains(ip_record));

    if should_have {
        assert!(
            has_ip,
            "should have {}/{}",
            ip_record.ip, ip_record.prefix_len
        );
    } else {
        assert!(
            !has_ip,
            "should not have {}/{}",
            ip_record.ip, ip_record.prefix_len
        );
    }
}
