//! OTP/HOTP/TOTP token calculation
//!

use hmac::{Hmac, Mac};
use sha1::Sha1;
use base64::Engine;

type HmacSha1 = Hmac<Sha1>;

/// OTP type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OtpType {
    Hotp,   // HMAC-based (counter)
    Totp,   // Time-based
}

/// OTP parameters parsed from an otpauth:// URI or KeePass OTP field.
#[derive(Debug, Clone)]
pub struct OtpParameters {
    pub otp_type: OtpType,
    pub secret: Vec<u8>,
    pub algorithm: OtpHashAlgorithm,
    pub digits: u32,
    pub period: u32,    // TOTP period in seconds (default 30)
    pub counter: u64,   // HOTP counter
    pub issuer: String,
    pub account: String,
}

/// Hash algorithm for OTP
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OtpHashAlgorithm {
    #[default]
    Sha1,
    Sha256,
    Sha512,
}

/// Token calculator.
pub struct TokenCalculator;

impl TokenCalculator {
    /// Calculate HOTP (HMAC-based OTP).
    pub fn hotp(secret: &[u8], counter: u64, digits: u32) -> u32 {
        let counter_bytes = counter.to_be_bytes();
        let mut mac = match HmacSha1::new_from_slice(secret) {
            Ok(m) => m,
            Err(_) => return 0,
        };
        mac.update(&counter_bytes);
        let result = mac.finalize().into_bytes();
        let hmac_bytes = result.as_slice();

        Self::truncate(hmac_bytes, digits)
    }

    /// Calculate TOTP (Time-based OTP).
    pub fn totp(secret: &[u8], time: u64, period: u32, digits: u32) -> u32 {
        let counter = time / period as u64;
        Self::hotp(secret, counter, digits)
    }

    /// Get current TOTP for the given parameters.
    pub fn current_totp(params: &OtpParameters) -> u32 {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        Self::totp(&params.secret, now, params.period, params.digits)
    }

    /// Truncate HMAC result to digits using dynamic truncation (RFC 4226).
    fn truncate(hmac: &[u8], digits: u32) -> u32 {
        let offset = (hmac[hmac.len() - 1] & 0x0F) as usize;
        let binary = ((hmac[offset] as u32 & 0x7F) << 24)
            | ((hmac[offset + 1] as u32) << 16)
            | ((hmac[offset + 2] as u32) << 8)
            | (hmac[offset + 3] as u32);

        let power = 10u32.pow(digits);
        binary % power
    }

    /// Format OTP code with leading zeros.
    pub fn format_code(code: u32, digits: u32) -> String {
        format!("{:0width$}", code, width = digits as usize)
    }
}

/// Parse an otpauth:// URI into OTP parameters.
pub fn parse_otpauth_uri(uri: &str) -> Option<OtpParameters> {
    if !uri.starts_with("otpauth://") {
        return None;
    }

    let uri = &uri["otpauth://".len()..];

    let (otp_type_str, rest) = uri.split_once('/')?;
    let otp_type = match otp_type_str.to_lowercase().as_str() {
        "hotp" => OtpType::Hotp,
        "totp" => OtpType::Totp,
        _ => return None,
    };

    let (label, params_str) = rest.split_once('?')?;
    let account = label.to_string();

    let mut secret = Vec::new();
    let mut digits = 6u32;
    let mut period = 30u32;
    let mut counter = 0u64;
    let mut algorithm = OtpHashAlgorithm::Sha1;
    let mut issuer = String::new();

    for param in params_str.split('&') {
        let (key, value) = param.split_once('=').unwrap_or(("", ""));
        match key {
            "secret" => {
                secret = base64::engine::general_purpose::STANDARD
                    .decode(value.replace(' ', ""))
                    .unwrap_or_default();
            }
            "digits" => digits = value.parse().unwrap_or(6),
            "period" => period = value.parse().unwrap_or(30),
            "counter" => counter = value.parse().unwrap_or(0),
            "algorithm" => {
                algorithm = match value.to_uppercase().as_str() {
                    "SHA256" => OtpHashAlgorithm::Sha256,
                    "SHA512" => OtpHashAlgorithm::Sha512,
                    _ => OtpHashAlgorithm::Sha1,
                };
            }
            "issuer" => issuer = value.to_string(),
            _ => {}
        }
    }

    Some(OtpParameters {
        otp_type,
        secret,
        algorithm,
        digits,
        period,
        counter,
        issuer,
        account,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hotp_rfc_4226_test_vector() {
        // RFC 4226 Appendix D test values
        let secret = b"12345678901234567890";
        assert_eq!(TokenCalculator::hotp(secret, 0, 6), 755224);
        assert_eq!(TokenCalculator::hotp(secret, 1, 6), 287082);
        assert_eq!(TokenCalculator::hotp(secret, 2, 6), 359152);
        assert_eq!(TokenCalculator::hotp(secret, 9, 6), 520489);
    }

    #[test]
    fn test_totp_format() {
        let code = 12345;
        assert_eq!(TokenCalculator::format_code(code, 6), "012345");
        assert_eq!(TokenCalculator::format_code(code, 8), "00012345");
    }

    #[test]
    fn test_parse_otpauth_uri() {
        let uri = "otpauth://totp/Test:user@example.com?secret=JBSWY3DPEHPK3PXP&issuer=Test&digits=6&period=30";
        let params = parse_otpauth_uri(uri).unwrap();
        assert_eq!(params.otp_type, OtpType::Totp);
        assert_eq!(params.digits, 6);
        assert_eq!(params.period, 30);
        assert_eq!(params.issuer, "Test");
    }

    #[test]
    fn test_totp_deterministic() {
        let secret = b"12345678901234567890";
        let time: u64 = 59;
        let result1 = TokenCalculator::totp(secret, time, 30, 6);
        let result2 = TokenCalculator::totp(secret, time, 30, 6);
        assert_eq!(result1, result2);
        // RFC 6238 test vector for SHA1 at time=59: 287082
        assert_eq!(result1, 287082);
    }

    #[test]
    fn test_parse_otpauth_hotp() {
        let uri = "otpauth://hotp/Test:user@example.com?secret=JBSWY3DPEHPK3PXP&issuer=Test&counter=42";
        let params = parse_otpauth_uri(uri).unwrap();
        assert_eq!(params.otp_type, OtpType::Hotp);
        assert_eq!(params.counter, 42);
        assert_eq!(params.issuer, "Test");
        assert!(!params.secret.is_empty());
    }

    #[test]
    fn test_parse_otpauth_sha256() {
        let uri = "otpauth://totp/Test:user@example.com?secret=JBSWY3DPEHPK3PXP&algorithm=SHA256&digits=8&period=60";
        let params = parse_otpauth_uri(uri).unwrap();
        assert_eq!(params.algorithm, OtpHashAlgorithm::Sha256);
        assert_eq!(params.digits, 8);
        assert_eq!(params.period, 60);
    }
}
