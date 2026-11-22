//! The readers of the About page (doc_settings.md SE22) that need no privilege: the system's name
//! and version from `/usr/lib/os-release`, the processor from `/proc/cpuinfo`, the memory from
//! `/proc/meminfo`, and the graphics adapter's name from `pci.ids`. The program reads the files;
//! these functions only parse their text.

/// The value of `key` in os-release text, without its quotes.
pub fn os_release<'a>(text: &'a str, key: &str) -> Option<&'a str> {
    text.lines().find_map(|line| {
        let (name, value) = line.split_once('=')?;
        if name.trim() != key {
            return None;
        }
        let value = value.trim();
        let unquoted = value
            .strip_prefix('"')
            .and_then(|v| v.strip_suffix('"'))
            .or_else(|| value.strip_prefix('\'').and_then(|v| v.strip_suffix('\'')))
            .unwrap_or(value);
        Some(unquoted)
    })
}

/// The first processor's model name and the number of logical processors.
pub fn processor(cpuinfo: &str) -> Option<(String, usize)> {
    let field = |line: &str, name: &str| {
        let (key, value) = line.split_once(':')?;
        (key.trim() == name).then(|| value.trim().to_owned())
    };
    let model = cpuinfo.lines().find_map(|line| field(line, "model name"))?;
    let count = cpuinfo
        .lines()
        .filter(|line| field(line, "processor").is_some())
        .count();
    Some((model, count.max(1)))
}

/// `MemTotal` in bytes.
pub fn memory_bytes(meminfo: &str) -> Option<u64> {
    let line = meminfo.lines().find(|line| line.starts_with("MemTotal:"))?;
    let kib: u64 = line.split_whitespace().nth(1)?.parse().ok()?;
    kib.checked_mul(1024)
}

/// The name `pci.ids` gives the device `vendor:device` (hexadecimal, as sysfs writes them with
/// or without `0x`): `Vendor Device`, or the vendor alone when the device is not listed.
pub fn pci_name(pci_ids: &str, vendor: &str, device: &str) -> Option<String> {
    let id = |text: &str| text.trim().trim_start_matches("0x").to_lowercase();
    let (vendor, device) = (id(vendor), id(device));
    let mut vendor_name: Option<&str> = None;
    for line in pci_ids.lines() {
        if line.starts_with('#') || line.is_empty() {
            continue;
        }
        if let Some(name) = vendor_name {
            if let Some(rest) = line.strip_prefix('\t') {
                if rest.starts_with('\t') {
                    continue;
                }
                if let Some((code, device_name)) = rest.split_once("  ") {
                    if code.to_lowercase() == device {
                        return Some(format!("{name} {}", device_name.trim()));
                    }
                }
                continue;
            }
            return Some(name.to_owned());
        }
        if let Some((code, name)) = line.split_once("  ") {
            if code.to_lowercase() == vendor {
                vendor_name = Some(name.trim());
            }
        }
    }
    vendor_name.map(str::to_owned)
}

/// A size in bytes as the page shows it, in decimal units with one decimal place: `15.5 GB`.
pub fn size(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["kB", "MB", "GB", "TB"];
    if bytes < 1000 {
        return format!("{bytes} B");
    }
    let mut value = bytes as f64;
    let mut name = "B";
    for unit in UNITS {
        if value < 1000.0 {
            break;
        }
        value /= 1000.0;
        name = unit;
    }
    format!("{value:.1} {name}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn os_release_values_lose_their_quotes() {
        let text = "NAME=\"Athanor\"\nVERSION='0.9.2'\nID=athanor\n# NAME=other\n";
        assert_eq!(os_release(text, "NAME"), Some("Athanor"));
        assert_eq!(os_release(text, "VERSION"), Some("0.9.2"));
        assert_eq!(os_release(text, "ID"), Some("athanor"));
        assert_eq!(os_release(text, "VARIANT"), None);
    }

    #[test]
    fn the_processor_and_the_memory_are_read_from_proc() {
        let cpuinfo = "processor\t: 0\nmodel name\t: Intel(R) Core(TM) i7-8550U CPU @ 1.80GHz\n\nprocessor\t: 1\nmodel name\t: Intel(R) Core(TM) i7-8550U CPU @ 1.80GHz\n";
        assert_eq!(
            processor(cpuinfo),
            Some(("Intel(R) Core(TM) i7-8550U CPU @ 1.80GHz".into(), 2))
        );
        assert_eq!(processor("flags : fpu\n"), None);
        assert_eq!(
            memory_bytes("MemTotal:       16163472 kB\nMemFree: 1 kB\n"),
            Some(16_551_395_328)
        );
        assert_eq!(memory_bytes("MemFree: 1 kB\n"), None);
    }

    #[test]
    fn a_graphics_adapter_is_named_from_pci_ids() {
        let ids = "# comment\n8086  Intel Corporation\n\t5917  UHD Graphics 620\n\t\t1028 0820  Subsystem\n\t3e92  CoffeeLake-S GT2 [UHD Graphics 630]\n10de  NVIDIA Corporation\n\t2684  AD102 [GeForce RTX 4090]\n";
        assert_eq!(
            pci_name(ids, "0x8086", "0x5917"),
            Some("Intel Corporation UHD Graphics 620".into())
        );
        assert_eq!(
            pci_name(ids, "10DE", "2684"),
            Some("NVIDIA Corporation AD102 [GeForce RTX 4090]".into())
        );
        assert_eq!(
            pci_name(ids, "0x8086", "0xffff"),
            Some("Intel Corporation".into())
        );
        assert_eq!(pci_name(ids, "0x1002", "0x0001"), None);
    }

    #[test]
    fn sizes_use_decimal_units() {
        assert_eq!(size(512), "512 B");
        assert_eq!(size(16_551_395_328), "16.6 GB");
        assert_eq!(size(512_110_190_592), "512.1 GB");
    }
}
