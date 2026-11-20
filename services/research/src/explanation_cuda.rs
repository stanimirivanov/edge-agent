//! Explicit local CUDA identity; never established by a model response.
use crate::{explanation_owned::digest, runtime::Result};
use serde::{Deserialize, Serialize};
use std::{fs, io::Read as _, path::Path};

/// Version-three CUDA execution descriptor, pinned into the deployment identity.
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Cuda {
    /// Lowercase Linux PCI domain/bus/device/function.
    pub pci: String,
    /// Exact NVIDIA GPU UUID selected through the clean runtime environment.
    pub uuid: String,
    /// Digest of the loaded kernel driver's version observation.
    pub driver: String,
    /// Explicit positive layer offload count; never automatic fitting.
    pub layers: u16,
    /// Explicit key/value cache precision: f16, q8_0 or q4_0.
    pub cache: String,
}
fn read(path: &Path) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    fs::File::open(path)?.take(65537).read_to_end(&mut bytes)?;
    if bytes.len() > 65536 {
        return Err("CUDA observation exceeds bound".into());
    }
    Ok(bytes)
}
impl Cuda {
    /// Refuse unbounded, ambiguous or path-like execution settings.
    pub fn validate(&self) -> Result<()> {
        let pci = self.pci.as_bytes();
        let uuid = self.uuid.strip_prefix("GPU-");
        let pci_ok = pci.len() == 12
            && pci.iter().enumerate().all(|(i, b)| match i {
                4 | 7 => *b == b':',
                10 => *b == b'.',
                11 => (b'0'..=b'7').contains(b),
                _ => b.is_ascii_digit() || (b'a'..=b'f').contains(b),
            });
        let uuid_ok = uuid.is_some_and(|v| {
            v.len() == 36
                && v.bytes().enumerate().all(|(i, b)| {
                    if [8, 13, 18, 23].contains(&i) {
                        b == b'-'
                    } else {
                        b.is_ascii_hexdigit()
                    }
                })
        });
        if !pci_ok
            || !uuid_ok
            || self.driver.len() != 64
            || !self
                .driver
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            || !(1..=64).contains(&self.layers)
            || !["f16", "q8_0", "q4_0"].contains(&self.cache.as_str())
        {
            return Err("invalid CUDA descriptor".into());
        }
        Ok(())
    }
    fn identity(&self, info: &str, driver: &[u8]) -> Result<u16> {
        self.validate()?;
        let field = |key: &str| {
            info.lines()
                .filter_map(|line| line.split_once(':'))
                .find_map(|(k, v)| (k.trim() == key).then_some(v.trim()))
        };
        if field("GPU UUID") != Some(self.uuid.as_str()) || digest(driver) != self.driver {
            return Err("CUDA identity changed".into());
        }
        let minor = field("Device Minor")
            .ok_or("CUDA device missing")?
            .parse::<u16>()?;
        if minor > 255 {
            return Err("CUDA device missing".into());
        }
        Ok(minor)
    }
    /// Check physical identity and loaded driver without querying a provider.
    pub fn observe(&self) -> Result<u16> {
        self.validate()?;
        let root = Path::new("/proc/driver/nvidia");
        self.identity(
            std::str::from_utf8(&read(
                &root.join("gpus").join(&self.pci).join("information"),
            )?)?,
            &read(&root.join("version"))?,
        )
    }
    /// Exact bounded arguments, shared with the owned supervisor contract.
    pub fn arguments(&self) -> Vec<String> {
        vec![
            "--fit".into(),
            "off".into(),
            "--device".into(),
            "CUDA0".into(),
            "--n-gpu-layers".into(),
            self.layers.to_string(),
            "--flash-attn".into(),
            "on".into(),
            "--cache-type-k".into(),
            self.cache.clone(),
            "--cache-type-v".into(),
            self.cache.clone(),
        ]
    }
    /// Recheck hardware, exact GPU selection and the process's actual device FD.
    pub fn verify_process(&self, pid: u32) -> Result<()> {
        let minor = self.observe()?;
        let root = std::path::PathBuf::from(format!("/proc/{pid}"));
        let environment = read(&root.join("environ"))?;
        for (key, value) in [
            ("CUDA_VISIBLE_DEVICES", self.uuid.as_str()),
            ("CUDA_DEVICE_ORDER", "PCI_BUS_ID"),
        ] {
            let prefix = format!("{key}=");
            let expected = format!("{key}={value}");
            let values: Vec<_> = environment
                .split(|b| *b == 0)
                .filter(|v| v.starts_with(prefix.as_bytes()))
                .collect();
            if values != [expected.as_bytes()] {
                return Err("CUDA environment changed".into());
            }
        }
        let target = std::path::PathBuf::from(format!("/dev/nvidia{minor}"));
        if !fs::read_dir(root.join("fd"))?
            .filter_map(std::result::Result::ok)
            .filter_map(|entry| fs::read_link(entry.path()).ok())
            .any(|path| path == target)
        {
            return Err("CUDA device not open".into());
        }
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn device_and_driver_drift_fail_closed() -> Result<()> {
        let mut value = Cuda {
            pci: "0000:01:00.0".into(),
            uuid: "GPU-11111111-1111-1111-1111-111111111111".into(),
            driver: digest(b"driver"),
            layers: 32,
            cache: "q4_0".into(),
        };
        let info = format!("GPU UUID: {}\nDevice Minor: 0\n", value.uuid);
        assert_eq!(value.identity(&info, b"driver")?, 0);
        assert!(value.identity(&info, b"other").is_err());
        assert!(
            value
                .identity("GPU UUID: other\nDevice Minor: 0\n", b"driver")
                .is_err()
        );
        value.pci = "../../other".into();
        assert!(value.validate().is_err());
        Ok(())
    }
}
