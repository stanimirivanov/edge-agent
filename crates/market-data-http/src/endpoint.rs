use crate::{HttpError, HttpErrorKind as K};
use std::collections::BTreeMap;
use url::Url;

pub(crate) struct Endpoint {
    pub(crate) url: Url,
    cursor: Option<String>,
    fixed: BTreeMap<String, String>,
}
impl Endpoint {
    pub(crate) fn new(text: &str, cursor: Option<&str>, loopback: bool) -> Result<Self, HttpError> {
        let url = Url::parse(text).map_err(|_| K::Configuration)?;
        if loopback
            && !url
                .host_str()
                .and_then(|h| h.parse::<std::net::IpAddr>().ok())
                .is_some_and(|ip| ip.is_loopback())
        {
            return Err(K::Configuration.into());
        }
        if text.len() > 4096
            || url.fragment().is_some()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.host_str().is_none()
            || !(url.scheme() == "https"
                || (loopback
                    && url.scheme() == "http"
                    && url
                        .host_str()
                        .and_then(|h| h.parse::<std::net::IpAddr>().ok())
                        .is_some_and(|ip| ip.is_loopback())))
            || cursor.is_some_and(|c| {
                c.is_empty()
                    || c.len() > 64
                    || !c.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
            })
        {
            return Err(K::Configuration.into());
        }
        let fixed = query(&url)?;
        if cursor.is_some_and(|c| fixed.contains_key(c)) {
            return Err(K::Configuration.into());
        }
        Ok(Self {
            url,
            cursor: cursor.map(str::to_owned),
            fixed,
        })
    }
    pub(crate) fn page(&self, link: Option<&str>) -> Result<Url, HttpError> {
        let Some(link) = link else {
            return Ok(self.url.clone());
        };
        if link.len() > 4096 || self.cursor.is_none() {
            return Err(K::Authority.into());
        }
        let url = self.url.join(link).map_err(|_| K::Authority)?;
        if url.origin() != self.url.origin()
            || url.path() != self.url.path()
            || url.fragment().is_some()
            || !url.username().is_empty()
            || url.password().is_some()
        {
            return Err(K::Authority.into());
        }
        let mut pairs = query(&url)?;
        let cursor = self.cursor.as_ref().ok_or(K::Authority)?;
        let value = pairs.remove(cursor).ok_or(K::Authority)?;
        if value.is_empty() || pairs != self.fixed {
            return Err(K::Authority.into());
        }
        Ok(url)
    }
}
fn query(url: &Url) -> Result<BTreeMap<String, String>, HttpError> {
    let mut pairs = BTreeMap::new();
    for (key, value) in url.query_pairs() {
        if matches!(
            key.to_ascii_lowercase().as_str(),
            "api_key" | "apikey" | "access_token" | "token" | "authorization" | "secret" | "key"
        ) {
            return Err(K::Authority.into());
        }
        if pairs.len() >= 16 || pairs.insert(key.into_owned(), value.into_owned()).is_some() {
            return Err(K::Authority.into());
        }
    }
    Ok(pairs)
}
