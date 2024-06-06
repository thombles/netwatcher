use crate::Error;
use std::fmt::Write;

pub(crate) fn format_mac(bytes: &[u8]) -> Result<String, Error> {
    let mut mac = String::with_capacity(bytes.len() * 3);
    for i in 0..bytes.len() {
        if i != 0 {
            write!(mac, ":").map_err(|_| Error::Internal)?;
        }
        write!(mac, "{:02X}", bytes[i]).map_err(|_| Error::Internal)?;
    }
    Ok(mac)
}
