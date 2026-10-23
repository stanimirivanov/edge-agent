//! Invented framing only; no vendor protocol, subscription, I/O or continuity qualification.
use edgeagent_market_data_application::{
    EarlyRouter, FrameKind, FrameMetadata, InstrumentRoute, RouteDecision, RoutingKey,
    RoutingScope, SelectionError as Error,
};
use edgeagent_market_data_domain::{FeedEpoch, FeedId, SourceId, Symbol};
use std::num::NonZeroU32;
type Result<T> = std::result::Result<T, Error>;

/// Result of routing one complete invented frame, never a P04 readiness result.
#[derive(Eq, PartialEq)]
pub enum RoutedFrame<T> {
    /// Selected payload decoded exactly once.
    Data(T),
    /// Known unselected payload was not decoded.
    Skipped,
    /// Explicit heartbeat must still reach the owner's continuity tracker.
    Heartbeat,
}
impl<T> std::fmt::Debug for RoutedFrame<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Data(_) => "RoutedFrame::Data(..)",
            Self::Skipped => "RoutedFrame::Skipped",
            Self::Heartbeat => "RoutedFrame::Heartbeat",
        })
    }
}
/// Encode an invented fixture frame, at most 1 MiB including headers.
/// Format EAS1: magic, kind:u8, source/feed:u128, epoch:u64, optional sequence
/// (tag:u8 + value:u64), optional symbol (length:u8 + bytes) or locate:u32,
/// body length:u32 and exact body bytes; integers are big-endian.
/// # Errors
/// Rejects excess bytes or payloads on control frames. No native feed format is implied.
pub fn synthetic_frame(header: &FrameMetadata, payload: &[u8]) -> Result<Vec<u8>> {
    if payload.len() > 1_048_485
        || (!matches!(header.kind, FrameKind::Data(_)) && !payload.is_empty())
    {
        return Err(Error::Framing);
    }
    let mut out = b"EAS1".to_vec();
    out.push(match &header.kind {
        FrameKind::Data(k) if k.as_symbol().is_some() => 0,
        FrameKind::Data(_) => 1,
        FrameKind::Heartbeat => 2,
        FrameKind::Reset => 3,
        FrameKind::DirectoryChanged => 4,
        FrameKind::AuthenticationFailure => 5,
    });
    out.extend_from_slice(&header.scope.source.get().to_be_bytes());
    out.extend_from_slice(&header.scope.feed.get().to_be_bytes());
    out.extend_from_slice(&header.scope.epoch.get().to_be_bytes());
    out.push(u8::from(header.sequence.is_some()));
    out.extend_from_slice(&header.sequence.unwrap_or(0).to_be_bytes());
    if let FrameKind::Data(key) = &header.kind {
        if let Some(s) = key.as_symbol() {
            out.push(s.len() as u8);
            out.extend_from_slice(s.as_bytes());
        } else {
            out.extend_from_slice(&key.as_locate().ok_or(Error::Framing)?.get().to_be_bytes());
        }
    }
    out.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    out.extend_from_slice(payload);
    if out.len() > 1_048_576 {
        return Err(Error::Framing);
    }
    Ok(out)
}
fn take<const N: usize>(input: &mut &[u8]) -> Result<[u8; N]> {
    let (head, tail) = input.split_at_checked(N).ok_or(Error::Framing)?;
    *input = tail;
    head.try_into().map_err(|_| Error::Framing)
}
fn parse(mut input: &[u8]) -> Result<(FrameMetadata, &[u8])> {
    if input.len() > 1_048_576 || take::<4>(&mut input)? != *b"EAS1" {
        return Err(Error::Framing);
    }
    let [kind] = take::<1>(&mut input)?;
    let scope = RoutingScope {
        source: SourceId::new(u128::from_be_bytes(take(&mut input)?))
            .map_err(|_| Error::Framing)?,
        feed: FeedId::new(u128::from_be_bytes(take(&mut input)?)).map_err(|_| Error::Framing)?,
        epoch: FeedEpoch::new(u64::from_be_bytes(take(&mut input)?)).map_err(|_| Error::Framing)?,
    };
    let [tag] = take::<1>(&mut input)?;
    let seq = u64::from_be_bytes(take(&mut input)?);
    let sequence = match (tag, seq) {
        (0, 0) => None,
        (1, n) => Some(n),
        _ => return Err(Error::Framing),
    };
    let kind = match kind {
        0 => {
            let [len] = take::<1>(&mut input)?;
            if len == 0 || len > 32 {
                return Err(Error::Framing);
            }
            let (symbol, tail) = input
                .split_at_checked(usize::from(len))
                .ok_or(Error::Framing)?;
            input = tail;
            let symbol = Symbol::new(std::str::from_utf8(symbol).map_err(|_| Error::Framing)?)
                .map_err(|_| Error::Framing)?;
            FrameKind::Data(RoutingKey::symbol(&symbol))
        }
        1 => FrameKind::Data(RoutingKey::locate(
            NonZeroU32::new(u32::from_be_bytes(take(&mut input)?)).ok_or(Error::Framing)?,
        )),
        2 => FrameKind::Heartbeat,
        3 => FrameKind::Reset,
        4 => FrameKind::DirectoryChanged,
        5 => FrameKind::AuthenticationFailure,
        _ => return Err(Error::Framing),
    };
    let size = u32::from_be_bytes(take(&mut input)?) as usize;
    if input.len() != size || (!matches!(kind, FrameKind::Data(_)) && size != 0) {
        return Err(Error::Framing);
    }
    Ok((
        FrameMetadata {
            scope,
            sequence,
            kind,
        },
        input,
    ))
}
/// Validate complete framing, then route header/control/sequence before invoking a decoder.
/// A dropped payload is never interpreted. The callback must validate selected data purely;
/// P04 continuity/readiness and provider conformance remain mandatory after routing.
/// # Errors
/// Latches invalidity on framing, routing or payload failure. No automatic recovery occurs.
pub fn route_synthetic_frame<'a, T>(
    router: &mut EarlyRouter<'a>,
    frame: &[u8],
    decode: impl FnOnce(InstrumentRoute<'a>, &[u8]) -> Result<T>,
) -> Result<RoutedFrame<T>> {
    let result = (|| {
        let (header, body) = parse(frame)?;
        match router.route(&header)? {
            RouteDecision::Decode(mapping) => decode(mapping, body).map(RoutedFrame::Data),
            RouteDecision::Skip => Ok(RoutedFrame::Skipped),
            RouteDecision::Heartbeat => Ok(RoutedFrame::Heartbeat),
        }
    })();
    if result.is_err() {
        router.invalidate();
    }
    result
}
