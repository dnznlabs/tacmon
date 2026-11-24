//! Formatting every monitor shares.

use std::time::{SystemTime, UNIX_EPOCH};

pub fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// Hundredths of a percent as "1.50%".
pub fn pct(bps: u64) -> String {
    format!("{}.{:02}%", bps / 100, bps % 100)
}

/// "1d02h", "3h12m", "4m05s", "12s"
pub fn dur(secs: u64) -> String {
    match secs {
        s if s >= 86400 => format!("{}d{:02}h", s / 86400, (s % 86400) / 3600),
        s if s >= 3600 => format!("{}h{:02}m", s / 3600, (s % 3600) / 60),
        s if s >= 60 => format!("{}m{:02}s", s / 60, s % 60),
        s => format!("{s}s"),
    }
}

/// "02cd4e8b…5455a8". Left whole when it is already short, or not ASCII (a byte index would split a character).
pub fn short(s: &str, head: usize, tail: usize) -> String {
    if s.len() <= head + tail + 1 || !s.is_ascii() {
        return s.to_string();
    }
    format!("{}…{}", &s[..head], &s[s.len() - tail..])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_percent_duration_and_long_hex() {
        let (bps, secs, hex) = (150, 3 * 3600 + 12 * 60, "02cd4e8b0afb64a927a2e762c15455a8");

        let out = (pct(bps), dur(secs), short(hex, 8, 6));

        assert_eq!(out, ("1.50%".to_string(), "3h12m".to_string(), "02cd4e8b…5455a8".to_string()));
    }
}
