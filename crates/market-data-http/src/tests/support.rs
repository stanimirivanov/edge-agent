use super::*;
use edgeagent_market_data_application::*;
use edgeagent_market_data_domain::*;
use std::{
    num::NonZeroU32,
    sync::{Arc, Mutex},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    sync::Notify,
    task::{JoinHandle, JoinSet},
};
pub type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;
pub fn nz(n: u32) -> TestResult<NonZeroU32> {
    NonZeroU32::new(n).ok_or("zero fixture".into())
}
pub fn at(ms: i64) -> TestResult<UtcInstant> {
    Ok(UtcInstant::from_unix_millis(ms)?)
}
pub const NOW: i64 = 172_800_000;
pub struct Fixture {
    pub request: AcquisitionRequest,
    rights: ReferenceHistory<RightsRecord>,
    reviews: ReferenceHistory<ProviderReview>,
}
impl Fixture {
    pub fn new(account: u128) -> TestResult<Self> {
        let scope = AccessScope {
            principal: PrincipalId::new(1)?,
            account: ProviderAccountId::new(account)?,
            source: SourceId::new(1)?,
            instrument: InstrumentId::new(1)?,
            endpoint: ReferenceId::new(1)?,
            audience: AudienceId::new(1)?,
            storage: StorageScopeId::new(1)?,
        };
        let contract = EvidenceContract::new(
            EvidenceKind::RawBar(BarCoverage::ProviderDay),
            MarketScope::Venue(Venue::Xnys),
            SemanticReference::new(ReferenceId::new(1)?, ReferenceRevision::new(1)?),
            TimestampPrecision::Milliseconds,
        )?;
        let effective = EffectiveInterval::new(at(0)?, at(NOW + 86_400_000)?)?;
        let right = RightsRecord {
            scope,
            contract,
            agreement: ReferenceId::new(1)?,
            effective,
            permissions: UsePermissions {
                automated: Permission::Allowed,
                derived: Permission::Allowed,
                canonical_storage: Permission::Allowed,
                audience: Permission::Allowed,
            },
            raw_storage: Permission::Unknown,
            canonical_retention_ms: nz(60_000)?,
            raw_retention_ms: None,
            delete_by: at(NOW + 60_000)?,
            embargo_ms: 0,
            status: ReviewStatus::Active,
        };
        let review = ProviderReview {
            scope,
            capability: ProviderCapability::new(
                scope.source,
                contract,
                DeliveryCapability::Snapshots,
                KnowledgeCapability::CaptureOnly,
            ),
            effective,
            status: ReviewStatus::Active,
        };
        let rights = ReferenceHistory::new(vec![ReferenceVersion::new(
            ReferenceId::new(1)?,
            ReferenceRevision::new(1)?,
            scope.source,
            at(0)?,
            [7; 32],
            right,
        )])?;
        // The generic version constructor is kept explicit for the distinct reviewed value type.
        let reviews = ReferenceHistory::new(vec![ReferenceVersion::new(
            ReferenceId::new(1)?,
            ReferenceRevision::new(1)?,
            scope.source,
            at(0)?,
            [7; 32],
            review,
        )])?;
        Ok(Self {
            request: AcquisitionRequest {
                id: AcquisitionId::new(1)?,
                scope,
                contract,
                range: DateRange::new(
                    SessionDate::new(1970, 1, 1)?,
                    SessionDate::new(1970, 1, 2)?,
                )?,
                knowledge: KnowledgeMode::Capture,
                admitted_at: at(NOW)?,
                deadline: at(NOW + 5000)?,
                budget: RequestBudget {
                    connect_ms: nz(500)?,
                    request_ms: nz(1000)?,
                    calls: nz(12)?,
                    pages: nz(4)?,
                    response_bytes: nz(1_048_576)?,
                    command_bytes: nz(8_388_608)?,
                },
                raw: RawRetention::Discard,
            },
            rights,
            reviews,
        })
    }
    pub fn admit<'a>(&'a self, cancel: &Cancellation) -> TestResult<AuthorizedAcquisition<'a>> {
        Ok(AuthorityCatalogs {
            rights: &self.rights,
            providers: &self.reviews,
        }
        .admit(&self.request, at(NOW)?, cancel)?)
    }
    pub fn config<'a>(&self, url: &'a str) -> TestResult<HttpConfig<'a>> {
        Ok(HttpConfig {
            scope: self.request.scope,
            endpoint: url,
            cursor_key: Some("cursor"),
            bearer: Some("invented-local-token"),
            connect_ms: self.request.budget.connect_ms,
            cancellation_poll_ms: nz(5)?,
        })
    }
    pub fn client(&self, url: &str) -> TestResult<HttpTransport> {
        Ok(HttpTransport::for_loopback(self.config(url)?)?)
    }
}
pub struct Server {
    pub url: String,
    pub requests: Arc<Mutex<Vec<Vec<u8>>>>,
    pub received: Arc<Notify>,
    task: JoinHandle<()>,
}
impl Server {
    pub async fn start(reply: Vec<u8>, stall: bool) -> TestResult<Self> {
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).await?;
        let url = format!("http://{}/data?symbol=INVENTED", listener.local_addr()?);
        let requests = Arc::new(Mutex::new(Vec::new()));
        let received = Arc::new(Notify::new());
        let (records, signal) = (requests.clone(), received.clone());
        let task = tokio::spawn(async move {
            let mut connections = JoinSet::new();
            loop {
                tokio::select! {
                    result=listener.accept()=>{let Ok((mut socket,_))=result else{break;};let reply=reply.clone();let records=records.clone();let signal=signal.clone();
                        connections.spawn(async move {let mut request=Vec::new();let mut buf=[0;1024];
                            while request.len()<8192&&!request.windows(4).any(|w|w==b"\r\n\r\n") { match socket.read(&mut buf).await{Ok(0)|Err(_)=>return,Ok(n)=>request.extend_from_slice(&buf[..n])} }
                            if let Ok(mut records)=records.lock(){records.push(request);}signal.notify_one();
                            if socket.write_all(&reply).await.is_ok()&&stall {std::future::pending::<()>().await;}
                        });
                    }
                    _=connections.join_next(),if !connections.is_empty()=>{}
                }
            }
        });
        Ok(Self {
            url,
            requests,
            received,
            task,
        })
    }
    pub fn count(&self) -> TestResult<usize> {
        Ok(self.requests.lock().map_err(|_| "fixture lock")?.len())
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}
pub fn response(status: u16, headers: &str, body: &[u8]) -> Vec<u8> {
    let mut reply=format!("HTTP/1.1 {status} Fixture\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n{headers}\r\n",body.len()).into_bytes();
    reply.extend_from_slice(body);
    reply
}
