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
}
