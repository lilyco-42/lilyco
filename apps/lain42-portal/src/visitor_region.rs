//! Resolves country from the transport peer and keeps only the result.

use std::{net::IpAddr, sync::LazyLock};

use maxminddb::{geoip2, Reader};

static DATABASE: LazyLock<Option<Reader<Vec<u8>>>> = LazyLock::new(|| {
    let path = std::env::var_os("LAIN_REGION_DB")?;
    match Reader::open_readfile(path) {
        Ok(reader) => Some(reader),
        Err(_) => {
            eprintln!("地区数据库无法读取，访问归属记为未知");
            None
        }
    }
});

pub(crate) fn enabled() -> bool {
    DATABASE.is_some()
}

pub(crate) fn lookup(ip: IpAddr) -> Option<String> {
    let country = DATABASE.as_ref()?.lookup(ip.to_canonical()).ok()?;
    let record = country.decode::<geoip2::Country>().ok()??;
    let code = record.country.iso_code?;
    (code.len() == 2 && code != "XX" && code.bytes().all(|byte| byte.is_ascii_uppercase()))
        .then(|| code.to_string())
}
