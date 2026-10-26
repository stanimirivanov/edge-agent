//! Single-owner frame I/O and bounded cancellation; no reconnect orchestration.
use super::*;
pub(super) async fn guard<T, A: StreamingAuthority>(
    authority: &A,
    session: &StreamingSession,
    last_utc: &mut i64,
    until: Instant,
    poll: Duration,
    cancellation: &Cancellation,
    future: impl Future<Output = Result<T>>,
) -> Result<T> {
    tokio::pin!(future);
    loop {
        check(authority, session, last_utc, until, cancellation)?;
        tokio::select! {biased;
            _=tokio::time::sleep_until(until)=>return Err(WssErrorKind::Timeout.into()),
            _=tokio::time::sleep(poll)=>{},
            value=&mut future=>{check(authority,session,last_utc,until,cancellation)?;return value;}
        }
    }
}
fn check<A: StreamingAuthority>(
    authority: &A,
    session: &StreamingSession,
    last_utc: &mut i64,
    until: Instant,
    cancellation: &Cancellation,
) -> Result<()> {
    if cancellation.is_cancelled() {
        return Err(WssErrorKind::Cancelled.into());
    }
    let now = authority.now();
    if now.unix_millis() < *last_utc {
        return Err(WssErrorKind::Authority.into());
    }
    *last_utc = now.unix_millis();
    if Instant::now() >= until {
        return Err(WssErrorKind::Timeout.into());
    }
    session.recheck(authority.catalogs(), now, cancellation)?;
    Ok(())
}
impl<L: StreamingLedger, A: StreamingAuthority> WssSession<'_, L, A> {
    /// Borrow the exact reserved generation for capture/publication composition.
    /// This witness never substitutes for an atomic current-generation check.
    #[must_use]
    pub const fn connection(&self) -> &StreamingConnection {
        &self.connection
    }
    /// Confirm each frame's durable charge before routing or payload decoding. No reassembly.
    /// # Errors
    /// Every failure terminates the handle. Malformed/oversized headers can fail before a complete
    /// arrival exists; advertised unread bytes are never claimed as actual received bytes.
    pub async fn receive(&mut self, cancellation: &Cancellation) -> Result<WssFrame> {
        let mut socket = self.socket.take().ok_or(WssErrorKind::Stopped)?;
        if self.pending_pong.is_some() {
            return Err(WssErrorKind::Protocol.into());
        }
        let until = self.deadline.min(self.idle_deadline);
        let mut frame = guard(
            self.authority,
            self.session,
            &mut self.last_utc,
            until,
            self.transport.config.poll,
            cancellation,
            async {
                socket
                    .read_frame()
                    .await
                    .map_err(|e| WssError::caused(WssErrorKind::Protocol, e))
            },
        )
        .await?;
        let received_monotonic = Instant::now();
        let received_at = edgeagent_market_data_domain::UtcInstant::from_unix_millis(self.last_utc)
            .map_err(|_| WssErrorKind::Authority)?;
        let bytes = u32::try_from(frame.payload.len()).map_err(|_| WssErrorKind::Budget)?;
        let ordinal = self.ordinal.checked_add(1).ok_or(WssErrorKind::Budget)?;
        let charge = guard(
            self.authority,
            self.session,
            &mut self.last_utc,
            until,
            self.transport.config.poll,
            cancellation,
            async {
                Ok(self
                    .ledger
                    .charge(
                        self.session,
                        &self.connection,
                        self.authority.catalogs(),
                        cancellation,
                        StreamingArrival { ordinal, bytes },
                    )
                    .await?)
            },
        )
        .await?;
        if charge.repeated
            || charge.usage.frames != ordinal
            || charge.usage.last_frame_bytes != Some(bytes)
        {
            return Err(WssErrorKind::Authority.into());
        }
        if !charge.within_limits {
            return Err(WssErrorKind::Budget.into());
        }
        self.ordinal = ordinal;
        self.idle_deadline = received_monotonic
            + Duration::from_millis(self.session.data().limits.idle_ms.get().into());
        let kind = match frame.opcode {
            OpCode::Text => WssFrameKind::Text,
            OpCode::Binary => WssFrameKind::Binary,
            OpCode::Continuation => WssFrameKind::Continuation,
            OpCode::Ping => WssFrameKind::Ping,
            OpCode::Pong => WssFrameKind::Pong,
            OpCode::Close => WssFrameKind::Close,
        };
        // The public codec accessor preserves the mask bit. The fixed 14-byte buffer
        // covers every RFC6455 header and satisfies fmt_head's documented invariant.
        let mut header = [0_u8; 14];
        frame.fmt_head(&mut header);
        if header[1] & 0x80 != 0 {
            return Err(WssErrorKind::Protocol.into());
        }
        if matches!(
            kind,
            WssFrameKind::Ping | WssFrameKind::Pong | WssFrameKind::Close
        ) {
            if bytes > 125 || !frame.fin || (kind == WssFrameKind::Close && bytes == 1) {
                return Err(WssErrorKind::Protocol.into());
            }
            if kind == WssFrameKind::Close && bytes >= 2 {
                // bytes is the exact payload length, checked >= 2 above.
                let code = u16::from_be_bytes([frame.payload[0], frame.payload[1]]);
                if !((1000..=1014).contains(&code) && !matches!(code, 1004..=1006)
                    || (3000..=4999).contains(&code))
                    || std::str::from_utf8(&frame.payload[2..]).is_err()
                {
                    return Err(WssErrorKind::Protocol.into());
                }
            }
        } else {
            let total = match (self.fragment_bytes, kind) {
                (None, WssFrameKind::Text | WssFrameKind::Binary) => bytes,
                (Some(prior), WssFrameKind::Continuation) => {
                    prior.checked_add(bytes).ok_or(WssErrorKind::Budget)?
                }
                _ => return Err(WssErrorKind::Protocol.into()),
            };
            if total > self.transport.config.message_bytes {
                return Err(WssErrorKind::Budget.into());
            }
            self.fragment_bytes = if frame.fin { None } else { Some(total) };
        }
        if kind == WssFrameKind::Ping {
            self.pending_pong = Some(frame.payload.to_vec());
        }
        let result = WssFrame {
            ordinal,
            received_at,
            kind,
            final_fragment: frame.fin,
            payload: frame.payload.to_vec(),
        };
        if !charge.usage.stopped && kind != WssFrameKind::Close {
            self.socket = Some(socket);
        }
        Ok(result)
    }
    /// Send one bounded complete text/binary frame or the exact owed pong; debit before I/O.
    /// # Errors
    /// Invalid kind/UTF-8/pong, exhausted writes, denial or uncertain I/O terminates the handle.
    pub async fn send(
        &mut self,
        kind: WssFrameKind,
        payload: &[u8],
        cancellation: &Cancellation,
    ) -> Result<()> {
        let mut socket = self.socket.take().ok_or(WssErrorKind::Stopped)?;
        // Bound caller input before UTF-8 scanning or copying into the codec.
        if self.writes >= self.transport.config.writes
            || payload.len() > self.transport.config.message_bytes as usize
            || payload.len()
                > self
                    .transport
                    .config
                    .write_bytes
                    .saturating_sub(self.written) as usize
        {
            return Err(WssErrorKind::Budget.into());
        }
        let opcode = match kind {
            WssFrameKind::Text => {
                std::str::from_utf8(payload).map_err(|_| WssErrorKind::Protocol)?;
                OpCode::Text
            }
            WssFrameKind::Binary => OpCode::Binary,
            WssFrameKind::Pong if self.pending_pong.as_deref() == Some(payload) => OpCode::Pong,
            _ => return Err(WssErrorKind::Protocol.into()),
        };
        if self.pending_pong.is_some() && opcode != OpCode::Pong {
            return Err(WssErrorKind::Protocol.into());
        }
        let bytes = u32::try_from(payload.len()).map_err(|_| WssErrorKind::Budget)?;
        self.writes = self.writes.checked_add(1).ok_or(WssErrorKind::Budget)?;
        self.written = self
            .written
            .checked_add(bytes)
            .ok_or(WssErrorKind::Budget)?;
        if self.writes > self.transport.config.writes
            || self.written > self.transport.config.write_bytes
            || bytes > self.transport.config.message_bytes
            || (opcode == OpCode::Pong && bytes > 125)
        {
            return Err(WssErrorKind::Budget.into());
        }
        let until = self.deadline.min(self.idle_deadline);
        guard(
            self.authority,
            self.session,
            &mut self.last_utc,
            until,
            self.transport.config.poll,
            cancellation,
            async {
                socket
                    .write_frame(Frame::new(true, opcode, None, Payload::Borrowed(payload)))
                    .await
                    .map_err(|e| WssError::caused(WssErrorKind::Protocol, e))
            },
        )
        .await?;
        self.pending_pong = None;
        self.socket = Some(socket);
        Ok(())
    }
}
