//! Linux procfs telemetry parsers. Collection is opt-in at the caller.

use thiserror::Error;
use waycontrol_core::{MemorySnapshot, NetworkInterface};

#[derive(Debug, Error, PartialEq, Eq)]
pub enum TelemetryError {
    #[error("invalid procfs data: {0}")]
    InvalidData(&'static str),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CpuTimes {
    pub total: u64,
    pub idle: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct NetworkRate {
    pub name: String,
    pub received_bytes_per_second: f64,
    pub transmitted_bytes_per_second: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CpuCoreTemperature {
    pub package_id: Option<u32>,
    pub core_id: u32,
    pub temperature_millidegrees: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StorageMount {
    pub source: String,
    pub mount_point: String,
    pub filesystem_type: String,
}

pub fn network_rates(
    previous: &[NetworkInterface],
    current: &[NetworkInterface],
    elapsed_seconds: f64,
) -> Vec<NetworkRate> {
    if !elapsed_seconds.is_finite() || elapsed_seconds <= 0.0 {
        return Vec::new();
    }
    current
        .iter()
        .filter_map(|now| {
            let before = previous.iter().find(|item| item.name == now.name)?;
            let received = now.received_bytes.checked_sub(before.received_bytes)?;
            let transmitted = now
                .transmitted_bytes
                .checked_sub(before.transmitted_bytes)?;
            Some(NetworkRate {
                name: now.name.clone(),
                received_bytes_per_second: received as f64 / elapsed_seconds,
                transmitted_bytes_per_second: transmitted as f64 / elapsed_seconds,
            })
        })
        .collect()
}

/// Parse aggregate CPU times without counting guest ticks twice.
pub fn parse_cpu_times(input: &str) -> Result<CpuTimes, TelemetryError> {
    let row = input
        .lines()
        .find(|line| line.starts_with("cpu "))
        .ok_or(TelemetryError::InvalidData("missing aggregate cpu row"))?;
    let values = row
        .split_whitespace()
        .skip(1)
        .map(|value| {
            value
                .parse::<u64>()
                .map_err(|_| TelemetryError::InvalidData("invalid cpu tick"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    if values.len() < 4 {
        return Err(TelemetryError::InvalidData("incomplete aggregate cpu row"));
    }
    // Linux reports guest and guest_nice as subsets of user and nice.
    let total = values.iter().take(8).try_fold(0_u64, |sum, value| {
        sum.checked_add(*value)
            .ok_or(TelemetryError::InvalidData("cpu tick overflow"))
    })?;
    let idle = values[3].saturating_add(values.get(4).copied().unwrap_or(0));
    Ok(CpuTimes { total, idle })
}

pub fn parse_cpu_total(input: &str) -> Result<u64, TelemetryError> {
    Ok(parse_cpu_times(input)?.total)
}

/// Parse Linux per-core CPU counters. Results retain the kernel's current row order.
pub fn parse_cpu_times_by_core(input: &str) -> Result<Vec<(String, CpuTimes)>, TelemetryError> {
    input
        .lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let name = fields.next()?;
            let suffix = name.strip_prefix("cpu")?;
            if suffix.is_empty() || !suffix.bytes().all(|byte| byte.is_ascii_digit()) {
                return None;
            }
            Some((name, fields))
        })
        .map(|(name, fields)| {
            let values = fields
                .map(|value| {
                    value
                        .parse::<u64>()
                        .map_err(|_| TelemetryError::InvalidData("invalid cpu tick"))
                })
                .collect::<Result<Vec<_>, _>>()?;
            if values.len() < 4 {
                return Err(TelemetryError::InvalidData("incomplete per-core cpu row"));
            }
            let total = values.iter().take(8).try_fold(0_u64, |sum, value| {
                sum.checked_add(*value)
                    .ok_or(TelemetryError::InvalidData("cpu tick overflow"))
            })?;
            let idle = values[3].saturating_add(values.get(4).copied().unwrap_or(0));
            Ok((name.to_owned(), CpuTimes { total, idle }))
        })
        .collect()
}

/// Calculate utilization for cores present in both samples, in current-sample order.
pub fn core_cpu_percentages(
    previous: &[(String, CpuTimes)],
    current: &[(String, CpuTimes)],
) -> Vec<(String, f64)> {
    current
        .iter()
        .filter_map(|(name, now)| {
            let (_, before) = previous
                .iter()
                .find(|(previous_name, _)| previous_name == name)?;
            cpu_percent(before.total, now.total, before.idle, now.idle)
                .map(|percent| (name.clone(), percent))
        })
        .collect()
}

/// Read the first three load averages from `/proc/loadavg`.
pub fn parse_load_average(input: &str) -> Result<[f64; 3], TelemetryError> {
    let mut fields = input.split_whitespace();
    let mut averages = [0.0; 3];
    for average in &mut averages {
        *average = fields
            .next()
            .and_then(|value| value.parse::<f64>().ok())
            .filter(|value| value.is_finite() && *value >= 0.0)
            .ok_or(TelemetryError::InvalidData("invalid load average"))?;
    }
    Ok(averages)
}

/// Parse a CPU frequency reported in kHz by Linux cpufreq sysfs.
pub fn parse_cpu_frequency_khz(input: &str) -> Option<u64> {
    input
        .trim()
        .parse::<u64>()
        .ok()
        .filter(|frequency| *frequency > 0)
}

/// Parse per-core temperature labels and millidegrees Celsius from `coretemp` hwmon.
pub fn parse_coretemp_temperature(
    package_label: Option<&str>,
    core_label: &str,
    input: &str,
) -> Option<CpuCoreTemperature> {
    let package_id = match package_label {
        Some(label) => Some(parse_hwmon_id(label, "Package id ")?),
        None => None,
    };
    let core_id = parse_hwmon_id(core_label, "Core ")?;
    let temperature_millidegrees = input.trim().parse::<i64>().ok()?;
    if !(-100_000..=250_000).contains(&temperature_millidegrees) {
        return None;
    }
    Some(CpuCoreTemperature {
        package_id,
        core_id,
        temperature_millidegrees,
    })
}

fn parse_hwmon_id(label: &str, prefix: &str) -> Option<u32> {
    let id = label.strip_prefix(prefix)?;
    if id.is_empty() || !id.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    id.parse().ok()
}

/// Parse mount entries suitable for disk-capacity reporting from `/proc/self/mounts`.
pub fn parse_storage_mounts(input: &str) -> Vec<StorageMount> {
    input
        .lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let source = fields.next()?;
            let mount_point = fields.next()?;
            let filesystem_type = fields.next()?;
            fields.next()?;
            fields.next()?;
            fields.next()?;
            if is_virtual_filesystem(filesystem_type) {
                return None;
            }
            Some(StorageMount {
                source: decode_mount_field(source),
                mount_point: decode_mount_field(mount_point),
                filesystem_type: filesystem_type.to_owned(),
            })
        })
        .collect()
}

fn is_virtual_filesystem(filesystem_type: &str) -> bool {
    matches!(
        filesystem_type,
        "autofs"
            | "binfmt_misc"
            | "bpf"
            | "cgroup"
            | "cgroup2"
            | "configfs"
            | "debugfs"
            | "devpts"
            | "devtmpfs"
            | "efivarfs"
            | "fusectl"
            | "hugetlbfs"
            | "mqueue"
            | "nsfs"
            | "proc"
            | "pstore"
            | "rpc_pipefs"
            | "securityfs"
            | "sysfs"
            | "tmpfs"
            | "ramfs"
            | "tracefs"
    )
}

fn decode_mount_field(field: &str) -> String {
    let bytes = field.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'\\'
            && index + 3 < bytes.len()
            && bytes[index + 1..index + 4]
                .iter()
                .all(|byte| (b'0'..=b'7').contains(byte))
        {
            let value = u16::from(bytes[index + 1] - b'0') * 64
                + u16::from(bytes[index + 2] - b'0') * 8
                + u16::from(bytes[index + 3] - b'0');
            if let Ok(value) = u8::try_from(value) {
                decoded.push(value);
                index += 4;
            } else {
                decoded.push(bytes[index]);
                index += 1;
            }
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8_lossy(&decoded).into_owned()
}

/// Calculate utilization between two cumulative samples. First/unchanged samples have no rate.
pub fn cpu_percent(
    previous_total: u64,
    current_total: u64,
    previous_idle: u64,
    current_idle: u64,
) -> Option<f64> {
    let total_delta = current_total.checked_sub(previous_total)?;
    let idle_delta = current_idle.checked_sub(previous_idle)?;
    if total_delta == 0 || idle_delta > total_delta {
        return None;
    }
    Some(((total_delta - idle_delta) as f64 / total_delta as f64) * 100.0)
}

pub fn parse_memory(input: &str) -> Result<MemorySnapshot, TelemetryError> {
    let mut values = std::collections::HashMap::new();
    for line in input.lines() {
        let Some((key, rest)) = line.split_once(':') else {
            continue;
        };
        let Some(value) = rest.split_whitespace().next() else {
            continue;
        };
        if let Ok(kib) = value.parse::<u64>() {
            values.insert(key, kib.saturating_mul(1024));
        }
    }
    let total = *values
        .get("MemTotal")
        .ok_or(TelemetryError::InvalidData("missing MemTotal"))?;
    let available = *values
        .get("MemAvailable")
        .ok_or(TelemetryError::InvalidData("missing MemAvailable"))?;
    let swap_total = *values
        .get("SwapTotal")
        .ok_or(TelemetryError::InvalidData("missing SwapTotal"))?;
    let swap_free = *values
        .get("SwapFree")
        .ok_or(TelemetryError::InvalidData("missing SwapFree"))?;
    if available > total || swap_free > swap_total {
        return Err(TelemetryError::InvalidData("free memory exceeds total"));
    }
    Ok(MemorySnapshot {
        total_bytes: total,
        available_bytes: available,
        used_bytes: total - available,
        swap_total_bytes: swap_total,
        swap_used_bytes: swap_total - swap_free,
    })
}

pub fn parse_network_interfaces(input: &str) -> Result<Vec<NetworkInterface>, TelemetryError> {
    let mut interfaces = Vec::new();
    for line in input.lines().skip_while(|line| !line.contains("|")).skip(2) {
        let Some((name, counters)) = line.split_once(':') else {
            continue;
        };
        let values = counters
            .split_whitespace()
            .map(|v| v.parse::<u64>())
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| TelemetryError::InvalidData("invalid network counter"))?;
        if values.len() < 9 {
            return Err(TelemetryError::InvalidData("incomplete network row"));
        }
        interfaces.push(NetworkInterface {
            name: name.trim().to_owned(),
            received_bytes: values[0],
            transmitted_bytes: values[8],
        });
    }
    Ok(interfaces)
}

/// Return interface names with an active default IPv4 or IPv6 route.
pub fn default_route_interfaces(
    ipv4_routes: &str,
    ipv6_routes: &str,
) -> std::collections::BTreeSet<String> {
    let mut interfaces = std::collections::BTreeSet::new();
    for line in ipv4_routes.lines().skip(1) {
        let mut fields = line.split_whitespace();
        let Some(name) = fields.next() else { continue };
        let Some(destination) = fields.next() else {
            continue;
        };
        let Some(_gateway) = fields.next() else {
            continue;
        };
        let Some(flags) = fields
            .next()
            .and_then(|value| u32::from_str_radix(value, 16).ok())
        else {
            continue;
        };
        let Some(_ref_count) = fields.next() else {
            continue;
        };
        let Some(_use_count) = fields.next() else {
            continue;
        };
        let Some(_metric) = fields.next() else {
            continue;
        };
        let Some(mask) = fields.next() else { continue };
        if destination == "00000000"
            && mask == "00000000"
            && flags & 1 != 0
            && valid_interface_name(name)
        {
            interfaces.insert(name.to_owned());
        }
    }

    for line in ipv6_routes.lines() {
        let fields = line.split_whitespace().collect::<Vec<_>>();
        if fields.len() < 10 {
            continue;
        }
        let flags = u32::from_str_radix(fields[8], 16).ok();
        if fields[0].bytes().all(|byte| byte == b'0')
            && fields[1] == "00"
            && flags.is_some_and(|value| value & 1 != 0)
            && valid_interface_name(fields[9])
        {
            interfaces.insert(fields[9].to_owned());
        }
    }
    interfaces
}

fn valid_interface_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() < 16
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-_.:".contains(&byte))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cpu_usage_uses_delta_and_returns_none_for_first_sample() {
        assert_eq!(cpu_percent(100, 200, 80, 130), Some(50.0));
        assert_eq!(cpu_percent(100, 100, 80, 80), None);
    }

    #[test]
    fn cpu_parser_includes_idle_and_iowait_without_double_counting_guest() {
        let sample =
            parse_cpu_times("cpu  10 2 3 20 5 1 1 0 4 1\ncpu0 1 0 0 2 0 0 0 0 0 0\n").unwrap();
        assert_eq!(sample.total, 42);
        assert_eq!(sample.idle, 25);
    }

    #[test]
    fn memory_uses_memavailable_and_reports_bytes() {
        let m = parse_memory(
            "MemTotal: 1000 kB\nMemAvailable: 250 kB\nSwapTotal: 80 kB\nSwapFree: 30 kB\n",
        )
        .unwrap();
        assert_eq!(m.used_bytes, 750 * 1024);
        assert_eq!(m.swap_used_bytes, 50 * 1024);
    }

    #[test]
    fn network_parser_extracts_receive_and_transmit_bytes() {
        let n = parse_network_interfaces("Inter-| Receive | Transmit\n face |bytes packets errs drop fifo frame compressed multicast|bytes packets errs drop fifo colls carrier compressed\n eth0: 11 0 0 0 0 0 0 0 22 0 0 0 0 0 0 0\n").unwrap();
        assert_eq!(n[0].name, "eth0");
        assert_eq!((n[0].received_bytes, n[0].transmitted_bytes), (11, 22));
    }

    #[test]
    fn network_rate_uses_counter_deltas_and_elapsed_seconds() {
        let previous = parse_network_interfaces("Inter-| Receive | Transmit\n face |bytes packets errs drop fifo frame compressed multicast|bytes packets errs drop fifo colls carrier compressed\n eth0: 100 0 0 0 0 0 0 0 200 0 0 0 0 0 0 0\n").unwrap();
        let current = parse_network_interfaces("Inter-| Receive | Transmit\n face |bytes packets errs drop fifo frame compressed multicast|bytes packets errs drop fifo colls carrier compressed\n eth0: 350 0 0 0 0 0 0 0 700 0 0 0 0 0 0 0\n").unwrap();
        let rates = network_rates(&previous, &current, 0.5);
        assert_eq!(rates[0].name, "eth0");
        assert_eq!(rates[0].received_bytes_per_second, 500.0);
        assert_eq!(rates[0].transmitted_bytes_per_second, 1000.0);
    }

    #[test]
    fn per_core_cpu_parser_and_rates_match_cores_by_id() {
        let previous = parse_cpu_times_by_core(
            "cpu 20 0 10 70 0 0 0 0\ncpu0 10 0 5 35 0 0 0 0\ncpu1 10 0 5 35 0 0 0 0\n",
        )
        .unwrap();
        let current = parse_cpu_times_by_core(
            "cpu 50 0 20 90 0 0 0 0\ncpu1 23 0 10 47 0 0 0 0\ncpu0 20 0 10 50 0 0 0 0\n",
        )
        .unwrap();

        assert_eq!(
            core_cpu_percentages(&previous, &current),
            vec![("cpu1".into(), 60.0), ("cpu0".into(), 50.0)]
        );
    }

    #[test]
    fn load_average_parser_reads_three_finite_linux_averages() {
        assert_eq!(
            parse_load_average("0.42 1.25 0.80 2/140 90210").unwrap(),
            [0.42, 1.25, 0.80]
        );
        assert_eq!(
            parse_load_average("0.42 NaN 0.80 2/140 90210"),
            Err(TelemetryError::InvalidData("invalid load average"))
        );
    }

    #[test]
    fn cpu_frequency_parser_accepts_positive_sysfs_khz_and_rejects_unavailable_values() {
        assert_eq!(parse_cpu_frequency_khz("2400000\n"), Some(2_400_000));
        assert_eq!(parse_cpu_frequency_khz("0\n"), None);
        assert_eq!(parse_cpu_frequency_khz("unavailable\n"), None);
    }

    #[test]
    fn parses_valid_per_core_temperatures_and_package_labels() {
        assert_eq!(
            parse_coretemp_temperature(Some("Package id 1"), "Core 12", "42875\n"),
            Some(CpuCoreTemperature {
                package_id: Some(1),
                core_id: 12,
                temperature_millidegrees: 42_875,
            })
        );
        assert_eq!(
            parse_coretemp_temperature(None, "Core 3", "39000\n"),
            Some(CpuCoreTemperature {
                package_id: None,
                core_id: 3,
                temperature_millidegrees: 39_000,
            })
        );
    }

    #[test]
    fn rejects_invalid_coretemp_labels_and_out_of_range_temperatures() {
        for (package, core, value) in [
            (Some("Package id x"), "Core 0", "39000"),
            (Some("Package id 0"), "Package id 0", "39000"),
            (Some("Package id 0"), "Core -1", "39000"),
            (Some("Package id 0"), "Core 0", "not-a-temperature"),
            (Some("Package id 0"), "Core 0", "300000"),
        ] {
            assert_eq!(parse_coretemp_temperature(package, core, value), None);
        }
    }

    #[test]
    fn storage_mount_parser_decodes_mount_escapes_and_skips_pseudo_filesystems() {
        let mounts = parse_storage_mounts(
            "/dev/nvme0n1p2 /mnt/My\\040Disk btrfs rw,relatime 0 0\nproc /proc proc rw,nosuid 0 0\nserver:/share /mnt/net\\040share nfs4 rw 0 0\n",
        );
        assert_eq!(
            mounts,
            vec![
                StorageMount {
                    source: "/dev/nvme0n1p2".into(),
                    mount_point: "/mnt/My Disk".into(),
                    filesystem_type: "btrfs".into(),
                },
                StorageMount {
                    source: "server:/share".into(),
                    mount_point: "/mnt/net share".into(),
                    filesystem_type: "nfs4".into(),
                },
            ]
        );
    }

    #[test]
    fn storage_mount_parser_skips_malformed_rows_and_decodes_backslashes() {
        let mounts = parse_storage_mounts("/dev/sda1 /mnt/a\\134b ext4 rw 0 0\nmalformed row\n");
        assert_eq!(mounts.len(), 1);
        assert_eq!(mounts[0].mount_point, "/mnt/a\\b");
    }

    #[test]
    fn default_route_parser_finds_only_active_ipv4_and_ipv6_default_interfaces() {
        let ipv4 = "Iface Destination Gateway Flags RefCnt Use Metric Mask\nwlan0 00000000 0101A8C0 0003 0 0 100 00000000\neth0 00000000 00000000 0002 0 0 0 00000000\neth1 00000000 00000000 0001 0 0 0 FF000000\n";
        let ipv6 = "00000000000000000000000000000000 00 00000000000000000000000000000000 00 00000000000000000000000000000000 00000000 00 00000000 00000001 wlan0\n00000000000000000000000000000000 00 00000000000000000000000000000000 00 00000000000000000000000000000000 00000000 00 00000000 00000000 eth1\n";
        assert_eq!(
            default_route_interfaces(ipv4, ipv6),
            ["wlan0".to_owned()].into_iter().collect()
        );
    }
}
