//! Canonical immutable explanation configuration, independent of strategy approval.
use crate::{explanation::Error, messaging::canonical_json, research::digest};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Profile size limit, checked before parsing.
pub const MAX_PROFILE_BYTES: usize = 2048;
/// Maximum duration encoded by the historical v1 profile.
pub const LEGACY_TIMEOUT_MS: u32 = 30_000;
/// Maximum configurable request duration for newly qualified profiles.
pub const MAX_TIMEOUT_MS: u32 = 600_000;

/// Requested runtime and model artifact identities. A profile does not attest
/// that a deployment matches them.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Deployment {
    /// Digest of the reviewed runtime deployment manifest.
    pub runtime: String,
    /// Digest of the exact model weights.
    pub weights: String,
}

/// Explicit bounded inference settings. Adapters must honor each pin.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    pub seed: u32,
    pub temperature_milli: u16,
    pub max_output_tokens: u16,
    /// Model request deadline in milliseconds, part of the approved identity.
    pub timeout_ms: u32,
}

/// The only supported tool configuration.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Tools {
    None,
}

/// Versioned configuration data. Any edit produces a different profile ID.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    pub deployment: Deployment,
    pub settings: Settings,
    /// Digest of the exact prompt bundle.
    pub prompt: String,
    /// Digest of the approved validator revision.
    pub validator: String,
    /// Digest of the approved renderer and templates.
    pub renderer: String,
    pub tools: Tools,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", content = "data", deny_unknown_fields)]
enum Wire<T> {
    #[serde(rename = "research.explanation.profile.v1")]
    V1(T),
    #[serde(rename = "research.explanation.profile.v2")]
    V2(T),
}

impl Profile {
    /// Validate pins and encode canonical JSON with one final LF.
    pub fn canonical_bytes(&self) -> Result<Vec<u8>, Error> {
        if [
            &self.deployment.runtime,
            &self.deployment.weights,
            &self.prompt,
            &self.validator,
            &self.renderer,
        ]
        .into_iter()
        .any(|pin| !digest(pin) || pin.bytes().all(|b| b == b'0'))
            || self.settings.temperature_milli > 2000
            || !(1..=2048).contains(&self.settings.max_output_tokens)
            || !(1..=MAX_TIMEOUT_MS).contains(&self.settings.timeout_ms)
        {
            return Err(Error);
        }
        let wire = if self.settings.timeout_ms > LEGACY_TIMEOUT_MS {
            Wire::V2(self)
        } else {
            Wire::V1(self)
        };
        let mut bytes = canonical_json::to_vec(&wire).map_err(|_| Error)?;
        bytes.push(b'\n');
        Ok(bytes)
    }

    /// Refuse malformed, oversized, unsupported, and noncanonical profiles.
    pub fn decode(bytes: &[u8]) -> Result<Self, Error> {
        if bytes.len() > MAX_PROFILE_BYTES {
            return Err(Error);
        }
        let (Wire::V1(profile) | Wire::V2(profile)): Wire<Self> =
            serde_json::from_slice(bytes).map_err(|_| Error)?;
        if profile.canonical_bytes()? != bytes {
            return Err(Error);
        }
        Ok(profile)
    }

    /// SHA-256 of the canonical profile bytes.
    pub fn id(&self) -> Result<String, Error> {
        Ok(Sha256::digest(self.canonical_bytes()?)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect())
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_bytes_are_strict_and_identity_covers_every_pin() -> Result<(), Error> {
        let profile = fixture();
        let bytes = profile.canonical_bytes()?;
        assert_eq!(Profile::decode(&bytes)?, profile);
        let id = profile.id()?;
        for changed in [
            Profile {
                deployment: Deployment {
                    runtime: "f".repeat(64),
                    ..profile.deployment.clone()
                },
                ..profile.clone()
            },
            Profile {
                deployment: Deployment {
                    weights: "f".repeat(64),
                    ..profile.deployment.clone()
                },
                ..profile.clone()
            },
            Profile {
                settings: Settings {
                    seed: 7,
                    ..profile.settings
                },
                ..profile.clone()
            },
            Profile {
                prompt: "f".repeat(64),
                ..profile.clone()
            },
            Profile {
                validator: "f".repeat(64),
                ..profile.clone()
            },
            Profile {
                renderer: "f".repeat(64),
                ..profile.clone()
            },
        ] {
            assert_ne!(changed.id()?, id);
        }
        let wire = std::str::from_utf8(&bytes).map_err(|_| Error)?;
        for invalid in [
            wire.replace("profile.v1", "profile.v2"),
            wire.replace("\"seed\":", "\"provider_option\":true,\"seed\":"),
            wire.replace("\"seed\":", "\"seed\":0,\"seed\":"),
            wire.replace("\"tools\":\"none\"", "\"tools\":[\"shell\"]"),
            wire.replace(&"b".repeat(64), "latest"),
            wire.replace("\"temperature_milli\":0", "\"temperature_milli\":2001"),
            wire.replace("\"max_output_tokens\":256", "\"max_output_tokens\":0"),
            wire.replace("\"timeout_ms\":1000", "\"timeout_ms\":30001"),
        ] {
            assert_ne!(invalid, wire);
            assert!(Profile::decode(invalid.as_bytes()).is_err());
        }
        assert!(Profile::decode(&bytes[..bytes.len() - 1]).is_err());
        assert!(Profile::decode(&vec![b' '; MAX_PROFILE_BYTES + 1]).is_err());
        Ok(())
    }

    #[test]
    fn extended_timeout_has_distinct_canonical_identity_and_bounds() -> Result<(), Error> {
        let mut profile = fixture();
        let old_bytes = profile.canonical_bytes()?;
        let old_id = profile.id()?;
        for timeout in [30_001, 60_000, 600_000] {
            profile.settings.timeout_ms = timeout;
            let bytes = profile.canonical_bytes()?;
            let wire = std::str::from_utf8(&bytes).map_err(|_| Error)?;
            assert!(wire.contains("research.explanation.profile.v2"));
            assert_eq!(Profile::decode(&bytes)?, profile);
            assert_ne!(profile.id()?, old_id);
            assert!(Profile::decode(wire.replace("profile.v2", "profile.v1").as_bytes()).is_err());
        }
        assert_eq!(Profile::decode(&old_bytes)?.id()?, old_id);
        let extended = String::from_utf8(profile.canonical_bytes()?).map_err(|_| Error)?;
        assert!(
            Profile::decode(
                extended
                    .replace("\"timeout_ms\":600000", "\"timeout_ms\":600001")
                    .as_bytes()
            )
            .is_err()
        );
        profile.settings.timeout_ms = 0;
        assert!(profile.canonical_bytes().is_err());
        Ok(())
    }

    fn fixture() -> Profile {
        Profile {
            deployment: Deployment {
                runtime: "a".repeat(64),
                weights: "b".repeat(64),
            },
            settings: Settings {
                seed: 1,
                temperature_milli: 0,
                max_output_tokens: 256,
                timeout_ms: 1000,
            },
            prompt: "c".repeat(64),
            validator: "d".repeat(64),
            renderer: "e".repeat(64),
            tools: Tools::None,
        }
    }
}
