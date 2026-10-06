//! Read-only diagnostic: never calls SetInterfaceDnsSettings or flushes DNS.
#[cfg(windows)]
fn main() {
    use windows::{core::GUID, Win32::NetworkManagement::IpHelper::*};
    let text = std::env::args().nth(1).expect("adapter GUID required");
    let guid =
        GUID::from_u128(u128::from_str_radix(&text.replace(['{', '}', '-'], ""), 16).unwrap());
    for flags in [0, 1] {
        let mut value = DNS_INTERFACE_SETTINGS3 {
            Version: 3,
            Flags: flags,
            ..Default::default()
        };
        let base = (&mut value as *mut DNS_INTERFACE_SETTINGS3).cast::<DNS_INTERFACE_SETTINGS>();
        let code = unsafe { GetInterfaceDnsSettings(guid, base) };
        println!(
            "requested={flags} code={} returnedFlags={}",
            code.0, value.Flags
        );
        if code.0 == 0 {
            for (label, ptr) in [
                ("NameServer", value.NameServer),
                ("ProfileNameServer", value.ProfileNameServer),
            ] {
                println!(
                    "{label}={:?}",
                    if ptr.is_null() {
                        None
                    } else {
                        unsafe { ptr.to_string().ok() }
                    }
                );
            }
            println!(
                "properties={} profileProperties={}",
                value.cServerProperties, value.cProfileServerProperties
            );
            unsafe { FreeInterfaceDnsSettings(base) };
        }
    }
}
#[cfg(not(windows))]
fn main() {}
