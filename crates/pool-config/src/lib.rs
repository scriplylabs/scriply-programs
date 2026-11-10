//! Reads a receivables pool's parameters from its config file
//! (`pools/*.toml`), for `create-pool` and the integration tests.
//!
//! ```ignore
//! let params = pool_config::load("pools/devnet.toml")?;
//! ```

use std::{fmt, path::Path, str::FromStr};

use anchor_lang::prelude::Pubkey;
use receivables_pool::state::{DepositorAttestation, PoolParams, RateMode, RiskConfig, RiskModel};
use serde::Deserialize;

/// The config file layout this crate reads.
pub const CONFIG_VERSION: u32 = 1;

/// Why a config file couldn't be read.
#[derive(Debug)]
pub enum ConfigError {
    Io(std::io::Error),
    Toml(toml::de::Error),
    UnsupportedVersion(u32),
    InvalidKey(String),
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "reading the config: {e}"),
            Self::Toml(e) => write!(f, "parsing the config: {e}"),
            Self::UnsupportedVersion(v) => {
                write!(f, "config version {v} is not {CONFIG_VERSION}")
            }
            Self::InvalidKey(k) => write!(f, "{k:?} is not a public key"),
        }
    }
}

impl std::error::Error for ConfigError {}

/// The pool parameters in the config file at `path`.
pub fn load(path: impl AsRef<Path>) -> Result<PoolParams, ConfigError> {
    let text = std::fs::read_to_string(path).map_err(ConfigError::Io)?;
    parse(&text)
}

/// The pool parameters in config file text.
pub fn parse(text: &str) -> Result<PoolParams, ConfigError> {
    let file: ConfigFile = toml::from_str(text).map_err(ConfigError::Toml)?;
    if file.version != CONFIG_VERSION {
        return Err(ConfigError::UnsupportedVersion(file.version));
    }
    file.to_params()
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ConfigFile {
    version: u32,
    network: String,
    signers: Vec<String>,
    epochs: Epochs,
    facts: Facts,
    loans: Loans,
    risk: Risk,
    #[serde(default)]
    depositors: Depositors,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Epochs {
    seconds: u64,
    mark_window_seconds: u64,
    maturities: Vec<u16>,
    grace: u16,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Facts {
    max_root_age_seconds: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Loans {
    cap_per_token_bps: u16,
    impaired_share_bps: u16,
    late_spread_bps: u16,
    rate_mode: RateModeName,
    risk_model: RiskModelName,
}

#[derive(Deserialize, Clone, Copy)]
#[serde(rename_all = "lowercase")]
enum RateModeName {
    Fixed,
    Variable,
}

#[derive(Deserialize, Clone, Copy)]
#[serde(rename_all = "lowercase")]
enum RiskModelName {
    V1,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Risk {
    reject: Reject,
    advance: Advance,
    rate: Rate,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Reject {
    late_after_days: u16,
    max_late_share_bps: u16,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Advance {
    base_bps: u16,
    max_bps: u16,
    weight_31_to_60_bps: u16,
    concentration_threshold_bps: u16,
    concentration_penalty_bps: u16,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Rate {
    base_bps: u16,
    min_bps: u16,
    max_bps: u16,
    past_due_spread_bps: u16,
    concentration_spread_bps: u16,
    dispute_spread_bps: u16,
    beyond_terms_spread_bps: u16,
    beyond_terms_cap_days: u16,
    not_on_time_spread_bps: u16,
    no_history_spread_bps: u16,
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct Depositors {
    attestation: Option<Attestation>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Attestation {
    credential: String,
    schema: String,
}

impl ConfigFile {
    fn to_params(&self) -> Result<PoolParams, ConfigError> {
        let attestation = match &self.depositors.attestation {
            Some(a) => Some(DepositorAttestation {
                credential: parse_key(&a.credential)?,
                schema: parse_key(&a.schema)?,
            }),
            None => None,
        };
        Ok(PoolParams {
            signers: self
                .signers
                .iter()
                .map(|s| parse_key(s))
                .collect::<Result<_, _>>()?,
            network: self.network.clone(),
            epoch_seconds: self.epochs.seconds,
            mark_window_seconds: self.epochs.mark_window_seconds,
            max_root_age: self.facts.max_root_age_seconds,
            maturities: self.epochs.maturities.clone(),
            grace_epochs: self.epochs.grace,
            impaired_share_bps: self.loans.impaired_share_bps,
            cap_per_token_bps: self.loans.cap_per_token_bps,
            late_spread_bps: self.loans.late_spread_bps,
            rate_mode: match self.loans.rate_mode {
                RateModeName::Fixed => RateMode::Fixed,
                RateModeName::Variable => RateMode::Variable,
            },
            risk_model: match self.loans.risk_model {
                RiskModelName::V1 => RiskModel::V1,
            },
            risk: RiskConfig {
                base_advance_bps: self.risk.advance.base_bps,
                max_advance_bps: self.risk.advance.max_bps,
                weight_31_to_60_bps: self.risk.advance.weight_31_to_60_bps,
                concentration_threshold_bps: self.risk.advance.concentration_threshold_bps,
                concentration_penalty_bps: self.risk.advance.concentration_penalty_bps,
                base_rate_bps: self.risk.rate.base_bps,
                min_rate_bps: self.risk.rate.min_bps,
                max_rate_bps: self.risk.rate.max_bps,
                past_due_spread_bps: self.risk.rate.past_due_spread_bps,
                concentration_spread_bps: self.risk.rate.concentration_spread_bps,
                dispute_spread_bps: self.risk.rate.dispute_spread_bps,
                late_after_days: self.risk.reject.late_after_days,
                max_late_share_bps: self.risk.reject.max_late_share_bps,
                beyond_terms_spread_bps: self.risk.rate.beyond_terms_spread_bps,
                beyond_terms_cap_days: self.risk.rate.beyond_terms_cap_days,
                not_on_time_spread_bps: self.risk.rate.not_on_time_spread_bps,
                no_history_spread_bps: self.risk.rate.no_history_spread_bps,
            },
            depositor_attestation: attestation,
        })
    }
}

fn parse_key(s: &str) -> Result<Pubkey, ConfigError> {
    Pubkey::from_str(s).map_err(|_| ConfigError::InvalidKey(s.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const DEVNET: &str = include_str!("../../../pools/devnet.toml");

    #[test]
    fn test_the_devnet_config_reads() {
        let p = parse(DEVNET).unwrap();
        assert_eq!(p.network, "sol-devnet");
        assert_eq!(p.maturities, vec![4, 13]);
        assert_eq!(p.risk.max_advance_bps, 9_000);
        assert_eq!(p.risk.late_after_days, 30);
        assert_eq!(p.risk.no_history_spread_bps, 200);
        assert_eq!(p.rate_mode, RateMode::Fixed);
        assert!(p.depositor_attestation.is_none());
    }

    #[test]
    fn test_an_unknown_field_is_refused() {
        let text = DEVNET.replace("grace = 2", "grace = 2\ngrase = 3");
        assert!(matches!(parse(&text), Err(ConfigError::Toml(_))));
    }

    #[test]
    fn test_another_version_is_refused() {
        let text = DEVNET.replace("version = 1", "version = 2");
        assert!(matches!(
            parse(&text),
            Err(ConfigError::UnsupportedVersion(2))
        ));
    }

    #[test]
    fn test_a_bad_signer_is_refused() {
        let text = DEVNET.replace("11111111111111111111111111111111", "nope");
        assert!(matches!(parse(&text), Err(ConfigError::InvalidKey(_))));
    }
}
