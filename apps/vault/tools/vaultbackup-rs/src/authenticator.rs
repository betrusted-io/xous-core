use std::str::FromStr;

use anyhow::{anyhow, bail, Result};
use protobuf::Message;

include!(concat!(env!("OUT_DIR"), "/protos/mod.rs"));

fn set_issuer(t: &mut backup::TotpEntry, issuer: String) {
    let mut issuer = issuer;
    issuer.push_str(":");
    if !t.name.starts_with(&issuer) {
        issuer.push_str(&t.name);
        t.name = issuer
    }
}

pub fn otpauth_to_entry(uri: &url::Url) -> Result<backup::TotpEntry, anyhow::Error> {
    let mut t = backup::TotpEntry::default();

    t.algorithm = backup::HashAlgorithms::SHA1;
    t.digit_count = 6;
    t.step_seconds = 30;
    t.name = uri.path()[1..].to_string();

    for (k, v) in uri.query_pairs() {
        match k.as_ref() {
            "secret" => {
                t.shared_secret = v.to_string();
            }
            "issuer" => {
                set_issuer(&mut t, v.to_string());
            }
            "algorithm" => {
                t.algorithm = backup::HashAlgorithms::from_str(&v)?;
            }
            "digits" => {
                t.digit_count = v.parse::<u32>()?;
            }
            "period" => {
                t.step_seconds = v.parse::<u64>()?;
            }
            k => {
                bail!("unexpected parameter {} in URI: {}", k, uri)
            }
        }
    }

    Ok(t)
}

fn migration_payload_to_entry(
    param: otpauth_migration::migration_payload::OtpParameters,
) -> Result<backup::TotpEntry, anyhow::Error> {
    match param.type_.enum_value_or_default() {
        otpauth_migration::migration_payload::OtpType::OTP_TYPE_TOTP => {
            let mut t = backup::TotpEntry::default();
            t.step_seconds = 30;
            match param.digits.enum_value_or_default() {
                otpauth_migration::migration_payload::DigitCount::DIGIT_COUNT_UNSPECIFIED => {
                    t.digit_count = 6
                }
                otpauth_migration::migration_payload::DigitCount::DIGIT_COUNT_SIX => t.digit_count = 6,
                otpauth_migration::migration_payload::DigitCount::DIGIT_COUNT_EIGHT => t.digit_count = 8,
            }
            match param.algorithm.enum_value_or_default() {
                otpauth_migration::migration_payload::Algorithm::ALGORITHM_UNSPECIFIED => {
                    t.algorithm = backup::HashAlgorithms::SHA1
                }
                otpauth_migration::migration_payload::Algorithm::ALGORITHM_SHA1 => {
                    t.algorithm = backup::HashAlgorithms::SHA1
                }
                otpauth_migration::migration_payload::Algorithm::ALGORITHM_SHA256 => {
                    t.algorithm = backup::HashAlgorithms::SHA256
                }
                otpauth_migration::migration_payload::Algorithm::ALGORITHM_SHA512 => {
                    t.algorithm = backup::HashAlgorithms::SHA512
                }
                otpauth_migration::migration_payload::Algorithm::ALGORITHM_MD5 => {
                    bail!("ALGORITHM_MD5 not supported")
                }
            }
            t.name = param.name;
            set_issuer(&mut t, param.issuer);
            t.shared_secret = base32::encode(base32::Alphabet::RFC4648 { padding: false }, &param.secret);
            Ok(t)
        }
        otpauth_migration::migration_payload::OtpType::OTP_TYPE_HOTP => {
            Err(anyhow!("OTP_TYPE_HOTP not supported"))
        }
        otpauth_migration::migration_payload::OtpType::OTP_TYPE_UNSPECIFIED => {
            Err(anyhow!("OTP_TYPE_UNSPECIFIED not supported"))
        }
    }
}

pub fn otpauth_migration_to_entries(uri: &url::Url) -> Result<Vec<backup::TotpEntry>, anyhow::Error> {
    let mut entries = Vec::new();

    for (k, v) in uri.query_pairs() {
        match k.as_ref() {
            "data" => {
                let data = base64::decode(&v.into_owned())?;
                let payload = otpauth_migration::MigrationPayload::parse_from_bytes(&data)?;
                for param in payload.otp_parameters {
                    entries.push(migration_payload_to_entry(param)?)
                }
            }
            k => {
                bail!("unexpected parameter {} in URI: {}", k, uri)
            }
        }
    }

    Ok(entries)
}
