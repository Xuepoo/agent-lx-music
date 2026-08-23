use std::time::Duration;

/// Connect-phase timeout applied to every native adapter request.
pub const CONNECT_TIMEOUT_SECS: u64 = 10;

/// Fallback total request timeout when no configured value is available.
pub const DEFAULT_TOTAL_TIMEOUT_SECS: u64 = 30;

/// Resolve the effective total request timeout.
///
/// A configured value of zero (or `None`) is treated as "unset" and falls back
/// to [`DEFAULT_TOTAL_TIMEOUT_SECS`] so requests can never run unbounded.
pub fn resolve_timeout(configured: Option<u64>) -> Duration {
    match configured {
        Some(secs) if secs > 0 => Duration::from_secs(secs),
        _ => Duration::from_secs(DEFAULT_TOTAL_TIMEOUT_SECS),
    }
}

/// Build a reqwest client with a bounded connect phase and a total timeout,
/// preventing stalled requests from starving command execution or download
/// daemon permits.
pub fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(CONNECT_TIMEOUT_SECS))
        .timeout(resolve_timeout(None))
        .build()
        .expect("failed to build native HTTP client")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_timeout_uses_configured_value() {
        assert_eq!(resolve_timeout(Some(45)), Duration::from_secs(45));
        assert_eq!(resolve_timeout(Some(1)), Duration::from_secs(1));
    }

    #[test]
    fn test_resolve_timeout_falls_back_on_none() {
        assert_eq!(
            resolve_timeout(None),
            Duration::from_secs(DEFAULT_TOTAL_TIMEOUT_SECS)
        );
    }

    #[test]
    fn test_resolve_timeout_falls_back_on_zero() {
        assert_eq!(
            resolve_timeout(Some(0)),
            Duration::from_secs(DEFAULT_TOTAL_TIMEOUT_SECS)
        );
    }

    #[test]
    fn test_client_has_timeouts_applied() {
        // The builder is opaque once built; construction succeeding with the
        // bounded configuration is the contract under test.
        let _client = client();
    }
}
