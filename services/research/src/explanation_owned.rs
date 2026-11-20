//! Exact reviewed local files and supervisor protocol for the guarded worker.
use crate::{
    explanation_cuda::Cuda,
    explanation_deployment::{Observation, Verifier},
    runtime::Result,
};
use edgeagent_contracts::{
    canonical_json_bytes,
    explanation_profile::{Deployment, Profile},
};
use serde::Deserialize;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Read as _, Write as _},
    path::Path,
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Pin {
    pub path: String,
    pub sha256: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    version: u8,
    pub runtime: Pin,
    pub weights: Pin,
    license: Pin,
    pub helper: Pin,
    prompt: Pin,
    profile: Pin,
    libraries: Vec<Pin>,
    hardware: serde_json::Value,
    threads: u16,
    context: u32,
    #[serde(default, deserialize_with = "decode_cuda")]
    cuda: Option<Cuda>,
}
fn decode_cuda<'de, D: serde::Deserializer<'de>>(
    value: D,
) -> std::result::Result<Option<Cuda>, D::Error> {
    Cuda::deserialize(value).map(Some)
}
pub fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
impl Pin {
    fn check(&self, limit: u64) -> Result<()> {
        if !Path::new(&self.path).is_absolute() {
            return Err("absolute reviewed file required".into());
        }
        let mut file = fs::File::open(&self.path)?;
        if file.metadata()?.len() > limit {
            return Err("file exceeds bound".into());
        }
        let mut hash = Sha256::new();
        let mut total = 0;
        let mut chunk = [0; 65536];
        loop {
            let n = file.read(&mut chunk)?;
            if n == 0 {
                break;
            }
            total += n as u64;
            if total > limit {
                return Err("file exceeds bound".into());
            }
            hash.update(&chunk[..n]);
        }
        if hash
            .finalize()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>()
            != self.sha256
        {
            return Err("reviewed file changed".into());
        }
        Ok(())
    }
}
impl Manifest {
    pub fn decode(bytes: &[u8]) -> Result<Self> {
        if bytes.len() > 32768 {
            return Err("manifest exceeds bound".into());
        }
        let value: Self = serde_json::from_slice(bytes)?;
        if !matches!(
            (value.version, value.cuda.as_ref()),
            (2, None) | (3, Some(_))
        ) || !(1..=256).contains(&value.threads)
            || !(512..=32768).contains(&value.context)
            || value.libraries.len() > 64
        {
            return Err("unsupported owned manifest".into());
        }
        if let Some(cuda) = &value.cuda {
            cuda.validate()?;
        }
        Ok(value)
    }
    pub fn environment() -> Result<(Self, String)> {
        let mut bytes = Vec::new();
        fs::File::open(std::env::var("EDGEAGENT_EXPLANATION_MANIFEST")?)?
            .take(32769)
            .read_to_end(&mut bytes)?;
        if bytes.len() > 32768 {
            return Err("manifest exceeds bound".into());
        }
        let pin = digest(&bytes);
        if std::env::var("EDGEAGENT_EXPLANATION_MANIFEST_SHA256")? != pin {
            return Err("manifest mismatch".into());
        }
        Ok((Self::decode(&bytes)?, pin))
    }
    pub fn profile(&self) -> Result<Profile> {
        self.profile.check(2048)?;
        Ok(Profile::decode(&fs::read(&self.profile.path)?)?)
    }
    pub fn verify(&self, profile: &Profile) -> Result<String> {
        for (pin, limit) in [
            (&self.runtime, 256 * 1024 * 1024),
            (&self.weights, 64 * 1024 * 1024 * 1024),
            (&self.helper, 512 * 1024 * 1024),
            (&self.license, 2 * 1024 * 1024),
            (&self.prompt, 4096),
        ] {
            pin.check(limit)?;
        }
        for pin in &self.libraries {
            pin.check(if self.version == 3 {
                2 * 1024 * 1024 * 1024
            } else {
                512 * 1024 * 1024
            })?;
        }
        if digest(&fs::read("/proc/self/exe")?) != self.helper.sha256 {
            return Err("foreign helper".into());
        }
        let cpu = fs::read("/proc/cpuinfo")?;
        let cut = cpu
            .windows(7)
            .position(|b| b == b"cpu MHz")
            .unwrap_or(cpu.len());
        let hardware = json!({"machine":std::env::consts::ARCH,"kernel":fs::read_to_string("/proc/sys/kernel/osrelease")?.trim(),
            "cpu_count":fs::read_to_string("/proc/stat")?.lines().filter(|l| l.starts_with("cpu") && l.as_bytes().get(3).is_some_and(u8::is_ascii_digit)).count(),
            "cpuinfo_sha256":digest(&cpu[..cut])});
        let mut value = json!({"hardware":hardware,"threads":self.threads,"context":self.context,
            "runtime":self.runtime.sha256,"weights":self.weights.sha256,"helper":self.helper.sha256,
            "libraries":self.libraries.iter().map(|p|&p.sha256).collect::<Vec<_>>(),
            "launcher":"owned-explanation-cpu-v2",
            "launcher_sha256":digest(include_bytes!("../../../scripts/run_explanation_owned.py")),
            "support_sha256":digest(include_bytes!("../../../scripts/qualify_explanation_model.py")),
            "hashing_sha256":digest(include_bytes!("../../../scripts/qualify_daily.py"))});
        if let Some(cuda) = &self.cuda {
            cuda.observe()?;
            value["launcher"] = "owned-explanation-cuda-v3".into();
            value["cuda"] = serde_json::to_value(cuda)?;
            value["gpu_support_sha256"] =
                digest(include_bytes!("../../../scripts/explanation_cuda.py")).into();
        }
        let mut bytes = canonical_json_bytes(&value)?;
        bytes.push(b'\n');
        if self.hardware != hardware
            || profile.deployment.runtime != digest(&bytes)
            || profile.deployment.weights != self.weights.sha256
            || profile.prompt != self.prompt.sha256
        {
            return Err("deployment binding mismatch".into());
        }
        Ok(fs::read_to_string(&self.prompt.path)?)
    }
    /// Exact invocation; no automatic fitting or backend fallback is admitted.
    pub fn command(&self, port: u16) -> Vec<String> {
        let mut command = vec![
            self.runtime.path.clone(),
            "--host".into(),
            "127.0.0.1".into(),
            "--port".into(),
            port.to_string(),
            "--model".into(),
            self.weights.path.clone(),
            "--alias".into(),
            self.weights.sha256.clone(),
            "--threads".into(),
            self.threads.to_string(),
            "--ctx-size".into(),
            self.context.to_string(),
            "--parallel".into(),
            "1".into(),
        ];
        if let Some(cuda) = &self.cuda {
            command.extend(cuda.arguments());
        } else {
            command.extend(["--device", "none", "--n-gpu-layers", "0"].map(String::from));
        }
        command.push("--log-disable".into());
        command
    }
    /// Construct the verifier with all manifest-required device guards.
    pub fn verifier(&self, observation: Observation, identity: Deployment) -> Result<Verifier> {
        let verifier = Verifier::new(observation, identity)?;
        match &self.cuda {
            Some(cuda) => verifier.with_cuda(cuda.clone()),
            None => Ok(verifier),
        }
    }
    pub fn observation(&self) -> Result<Observation> {
        std::io::stdout()
            .write_all(b"{\"kind\":\"research.explanation-runtime.required.v2\"}\n")?;
        std::io::stdout().flush()?;
        let mut bytes = Vec::new();
        std::io::stdin().take(16385).read_to_end(&mut bytes)?;
        if bytes.len() > 16384 {
            return Err("runtime observation exceeds bound".into());
        }
        let observation: Observation = serde_json::from_slice(&bytes)?;
        let expected = self.command(observation.port);
        if observation.executable != self.runtime.sha256 || observation.command != expected {
            return Err("runtime command mismatch".into());
        }
        Ok(observation)
    }
}
