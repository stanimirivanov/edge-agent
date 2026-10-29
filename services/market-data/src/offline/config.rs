use super::*;
use std::{
    fmt,
    fs::{File, OpenOptions},
    path::PathBuf,
};

#[derive(Debug)]
pub(super) struct ConfigError(pub &'static str);
impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}
impl std::error::Error for ConfigError {}
pub(super) const MARKER: &[u8] = b"edgeagent.invented.offline-profile.v1\n";
pub(super) struct Config {
    pub command: String,
    pub database: String,
    pub root: std::path::PathBuf,
    pub request: Option<u128>,
    pub recipe: super::fixture::Recipe,
    pub broker: Option<String>,
    pub stream: Option<String>,
    pub consumer: Option<String>,
    pub messages: u8,
    pub confirmed: BTreeSet<EvidenceId>,
}
fn required(name: &'static str) -> Result<String> {
    std::env::var(name)
        .ok()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| ConfigError(name).into())
}
fn optional(name: &'static str) -> Result<Option<String>> {
    match std::env::var(name) {
        Ok(value) => Ok(Some(value)),
        Err(std::env::VarError::NotPresent) => Ok(None),
        Err(std::env::VarError::NotUnicode(_)) => {
            Err(ConfigError("offline mode/provider configuration must be UTF-8").into())
        }
    }
}
pub(super) fn mode(mock: Option<&str>, provider: Option<&str>) -> Result {
    edgeagent_market_data_replay::OfflineMode::from_mock_flag(mock).map_err(|_| {
        ConfigError("USE_MOCK must be absent, 1 or true; live mode is not implemented")
    })?;
    if !matches!(provider, None | Some("synthetic-daily-v1")) {
        return Err(ConfigError(
            "only provider synthetic-daily-v1 is installed; vendor activation is disabled",
        )
        .into());
    }
    Ok(())
}
pub(super) fn local_endpoint(value: &str, schemes: &[&str]) -> Result {
    let parsed =
        url::Url::parse(value).map_err(|_| ConfigError("invalid local infrastructure URL"))?;
    if !schemes.contains(&parsed.scheme())
        // PostgreSQL query options can override the authority's host. This profile
        // accepts no routing/options escape hatch, even for trusted local callers.
        || parsed.query().is_some()
        || parsed.fragment().is_some()
        || !matches!(
            parsed.host_str(),
            Some("localhost" | "127.0.0.1" | "[::1]" | "::1")
        )
    {
        return Err(ConfigError(
            "offline infrastructure endpoints must use loopback hosts and the expected scheme",
        )
        .into());
    }
    Ok(())
}
impl Config {
    pub fn read(args: &[String]) -> Result<Self> {
        mode(
            optional("USE_MOCK")?.as_deref(),
            optional("EDGEAGENT_MARKET_DATA_PROVIDER")?.as_deref(),
        )?;
        let command = args
            .first()
            .ok_or(ConfigError("missing offline command"))?
            .clone();
        let mut request = None;
        let mut recipe = super::fixture::Recipe::Single;
        let mut confirmed = BTreeSet::new();
        match command.as_str() {
            "offline-seed" if args.len() == 2 || args.len() == 3 => {
                if let Some(name) = args.get(2) { recipe = super::fixture::Recipe::parse(name)?; }
                let id = EvidenceId::new(&args[1]).map_err(|_| ConfigError("seed requires one canonical nonzero hexadecimal request identity"))?;
                request = Some(u128::from_str_radix(id.as_str(), 16)?);
            }
            "offline-research-once" | "offline-research-worker" if args.len() == 2 => {
                let id=EvidenceId::new(&args[1]).map_err(|_|ConfigError("research requires one owner-installed acquisition ID"))?;
                request=Some(u128::from_str_radix(id.as_str(),16)?);
                let scope=required("EDGEAGENT_OFFLINE_RESEARCH_SCOPE")?;
                if scope.is_empty() || scope.len()>64 || !scope.bytes().all(|b|b.is_ascii_alphanumeric() || b"._-".contains(&b)) {return Err(ConfigError("invalid research operator scope").into());}
            }
            "offline-maintain" if (3..=18).contains(&args.len()) && args.get(1).is_some_and(|v| v == "--owner-confirmed-quiescence") => {
                for id in &args[2..] { confirmed.insert(EvidenceId::new(id)?); }
            }
            "offline-init" | "offline-worker" | "offline-relay" if args.len() == 1 => (),
            _ => return Err(ConfigError("invalid offline command arguments; maintenance requires explicit owner-confirmed request identities").into()),
        }
        let database = required("EDGEAGENT_POSTGRES_URL")?;
        local_endpoint(&database, &["postgres", "postgresql"])?;
        let root = PathBuf::from(required("EDGEAGENT_OFFLINE_ROOT")?);
        let (broker, stream, consumer, messages) = if matches!(
            command.as_str(),
            "offline-worker" | "offline-relay" | "offline-research-worker"
        ) {
            let broker = required("EDGEAGENT_NATS_URL")?;
            local_endpoint(&broker, &["nats", "tls"])?;
            let stream = if matches!(
                command.as_str(),
                "offline-worker" | "offline-research-worker"
            ) {
                required("EDGEAGENT_OFFLINE_STREAM")?
            } else {
                String::new()
            };
            let consumer = if matches!(
                command.as_str(),
                "offline-worker" | "offline-research-worker"
            ) {
                required("EDGEAGENT_OFFLINE_CONSUMER")?
            } else {
                String::new()
            };
            if [&stream, &consumer].iter().any(|s| {
                s.len() > 64
                    || !s
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
            }) {
                return Err(ConfigError("invalid offline stream/consumer identity").into());
            }
            let messages = required("EDGEAGENT_OFFLINE_MAX_MESSAGES")?
                .parse::<u8>()
                .map_err(|_| ConfigError("EDGEAGENT_OFFLINE_MAX_MESSAGES must be 1..=16"))?;
            if !(1..=16).contains(&messages) {
                return Err(ConfigError("EDGEAGENT_OFFLINE_MAX_MESSAGES must be 1..=16").into());
            }
            (Some(broker), Some(stream), Some(consumer), messages)
        } else {
            (None, None, None, 0)
        };
        Ok(Self {
            command,
            database,
            root,
            request,
            recipe,
            broker,
            stream,
            consumer,
            messages,
            confirmed,
        })
    }
    pub fn lock(&self) -> Result<File> {
        if self.command == "offline-init" {
            std::fs::create_dir_all(&self.root)?;
        }
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(self.root.join("offline.lock"))?;
        lock.try_lock().map_err(|_| {
            ConfigError("offline profile already has an owner or locking is unsupported")
        })?;
        if self.command == "offline-init" {
            if std::fs::read_dir(&self.root)?.any(|entry| match entry {
                Ok(e) => e.file_name() != "offline.lock",
                Err(_) => true,
            }) {
                return Err(
                    ConfigError("offline-init requires an empty dedicated directory").into(),
                );
            }
        } else if std::fs::read(self.root.join("profile"))? != MARKER
            || !self.root.join("objects").is_dir()
        {
            return Err(ConfigError("offline profile is uninitialized or incompatible").into());
        }
        Ok(lock)
    }
    pub fn schema(&self) -> Result<String> {
        let root = std::fs::canonicalize(&self.root)?;
        let path = root
            .to_str()
            .ok_or(ConfigError("offline root must be UTF-8"))?;
        Ok(format!(
            "edgeagent_offline_{}",
            &DailyEvidenceArtifact::hash(path.as_bytes()).as_str()[..32]
        ))
    }
}
