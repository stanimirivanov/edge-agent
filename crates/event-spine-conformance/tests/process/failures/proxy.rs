//! Loopback-only, single-worker NATS frame proxy. No mock settlement results.

use std::sync::{Arc, Mutex};
use tokio::{
    io::{AsyncBufRead, AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    net::{TcpListener, TcpStream},
    task::JoinHandle,
};

use super::{TestError, TestResult};

#[derive(Clone, Copy)]
pub enum Loss {
    Request,
    Confirmation,
}

#[derive(Default, Debug, Clone, Copy, PartialEq, Eq)]
pub struct Evidence {
    pub requests: u32,
    pub forwarded: u32,
    pub confirmations_dropped: u32,
}

struct State {
    loss: Loss,
    reply: Option<String>,
    evidence: Evidence,
}

pub struct Proxy {
    pub url: String,
    state: Arc<Mutex<State>>,
    changed: Arc<tokio::sync::Notify>,
    task: JoinHandle<TestResult>,
}

impl Proxy {
    pub async fn start(upstream: &str, loss: Loss) -> Result<Self, TestError> {
        let address = upstream
            .strip_prefix("nats://127.0.0.1:")
            .ok_or("proxy upstream must be owned loopback")?;
        let port: u16 = address.parse()?;
        let listener = TcpListener::bind(("127.0.0.1", 0)).await?;
        let url = format!("nats://{}", listener.local_addr()?);
        let state = Arc::new(Mutex::new(State {
            loss,
            reply: None,
            evidence: Evidence::default(),
        }));
        let observed = state.clone();
        let changed = Arc::new(tokio::sync::Notify::new());
        let notified = changed.clone();
        let task = tokio::spawn(async move {
            let (downstream, _) = listener.accept().await?;
            let upstream = TcpStream::connect(("127.0.0.1", port)).await?;
            let (client_read, mut client_write) = downstream.into_split();
            let (server_read, mut server_write) = upstream.into_split();
            let mut client_read = BufReader::new(client_read);
            let mut server_read = BufReader::new(server_read);
            let from_client = async {
                while let Some(frame) = Frame::read(&mut client_read).await? {
                    let drop_frame = client_drop(&frame, &observed)?;
                    notified.notify_one();
                    if drop_frame {
                        continue;
                    }
                    server_write.write_all(&frame.line).await?;
                    server_write.write_all(&frame.body).await?;
                }
                Ok::<(), TestError>(())
            };
            let from_server = async {
                while let Some(frame) = Frame::read(&mut server_read).await? {
                    let drop_frame = server_drop(&frame, &observed)?;
                    notified.notify_one();
                    if drop_frame {
                        continue;
                    }
                    client_write.write_all(&frame.line).await?;
                    client_write.write_all(&frame.body).await?;
                }
                Ok::<(), TestError>(())
            };
            // EOF in either direction ends this one-worker connection. Drop
            // both halves; never leave a detached forwarding task or listener.
            tokio::select! { result = from_client => result, result = from_server => result }
        });
        Ok(Self {
            url,
            state,
            changed,
            task,
        })
    }

    #[allow(
        dead_code,
        reason = "M03 observes the wire barrier before killing the real worker"
    )]
    pub async fn observed(&self) -> Result<Evidence, TestError> {
        tokio::time::timeout(std::time::Duration::from_secs(30), async {
            loop {
                let notified = self.changed.notified();
                {
                    let state = self.state.lock().map_err(|_| "proxy evidence poisoned")?;
                    if state.evidence.requests > 0
                        && (matches!(state.loss, Loss::Request)
                            || state.evidence.confirmations_dropped > 0)
                    {
                        return Ok(state.evidence);
                    }
                }
                notified.await;
            }
        })
        .await?
    }

    pub async fn finish(mut self) -> Result<Evidence, TestError> {
        let result =
            tokio::time::timeout(std::time::Duration::from_secs(5), &mut self.task).await??;
        // TerminateProcess/SIGKILL can produce reset/broken-pipe instead of EOF.
        // Accept only socket teardown, never framing/authentication failures.
        if let Err(error) = result {
            let closed = error.downcast_ref::<std::io::Error>().is_some_and(|error| {
                matches!(
                    error.kind(),
                    std::io::ErrorKind::ConnectionReset | std::io::ErrorKind::BrokenPipe
                )
            });
            if !closed {
                return Err(error);
            }
        }
        Ok(self
            .state
            .lock()
            .map_err(|_| "proxy evidence poisoned")?
            .evidence)
    }
}

impl Drop for Proxy {
    fn drop(&mut self) {
        self.task.abort();
    }
}

fn client_drop(frame: &Frame, state: &Mutex<State>) -> Result<bool, TestError> {
    let fields = frame.fields()?;
    if fields.first() != Some(&"PUB") || !fields.get(1).is_some_and(|s| s.starts_with("$JS.ACK.")) {
        return Ok(false);
    }
    if fields.len() != 4 || frame.body != b"+ACK\r\n" {
        return Err("unexpected ACK request framing".into());
    }
    let mut state = state.lock().map_err(|_| "proxy evidence poisoned")?;
    state.evidence.requests += 1;
    state.reply = Some(fields[2].to_owned());
    if matches!(state.loss, Loss::Request) {
        return Ok(true);
    }
    state.evidence.forwarded += 1;
    Ok(false)
}

fn server_drop(frame: &Frame, state: &Mutex<State>) -> Result<bool, TestError> {
    let fields = frame.fields()?;
    let mut state = state.lock().map_err(|_| "proxy evidence poisoned")?;
    if matches!(state.loss, Loss::Confirmation)
        && fields.first() == Some(&"MSG")
        && fields
            .get(1)
            .is_some_and(|s| state.reply.as_deref() == Some(*s))
    {
        if frame.body != b"\r\n" {
            return Err("unexpected ACK confirmation".into());
        }
        state.evidence.confirmations_dropped += 1;
        return Ok(true);
    }
    Ok(false)
}

struct Frame {
    line: Vec<u8>,
    body: Vec<u8>,
}

impl Frame {
    fn fields(&self) -> Result<Vec<&str>, TestError> {
        Ok(std::str::from_utf8(&self.line)?
            .split_ascii_whitespace()
            .collect())
    }

    async fn read(reader: &mut (impl AsyncBufRead + Unpin)) -> Result<Option<Self>, TestError> {
        const LINE: u64 = 4096;
        const BODY: usize = edgeagent_contracts::MAX_PORTABLE_MESSAGE_BYTES + 4096;
        let mut frame = Self {
            line: Vec::new(),
            body: Vec::new(),
        };
        let length = (&mut *reader)
            .take(LINE + 1)
            .read_until(b'\n', &mut frame.line)
            .await?;
        if length == 0 {
            return Ok(None);
        }
        if u64::try_from(length)? > LINE || !frame.line.ends_with(b"\r\n") {
            return Err("proxy control frame exceeds bounds or is truncated".into());
        }
        let fields = frame.fields()?;
        if matches!(fields.first(), Some(&"PUB" | &"HPUB" | &"MSG" | &"HMSG")) {
            let size: usize = fields.last().ok_or("missing frame length")?.parse()?;
            if size > BODY {
                return Err("proxy body exceeds bound".into());
            }
            if matches!(fields.first(), Some(&"HPUB" | &"HMSG")) {
                let headers: usize = fields
                    .get(fields.len().checked_sub(2).ok_or("missing headers")?)
                    .ok_or("missing headers")?
                    .parse()?;
                if headers > size {
                    return Err("invalid header length".into());
                }
            }
            frame.body.resize(size + 2, 0);
            reader.read_exact(&mut frame.body).await?;
            if !frame.body.ends_with(b"\r\n") {
                return Err("invalid body terminator".into());
            }
        }
        Ok(Some(frame))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn framing_preserves_fragmented_binary_and_header_frames_and_rejects_bounds() -> TestResult
    {
        for input in [
            b"PUB a b 4\r\n\0\r\nX\r\n".as_slice(),
            b"HPUB a b 3 5\r\nabc\0X\r\n",
            b"MSG a 1 0\r\n\r\n",
            b"HMSG a 1 3 3\r\nabc\r\n",
        ] {
            let (mut writer, reader) = tokio::io::duplex(1);
            let (write, read) = tokio::join!(async { writer.write_all(input).await }, async {
                Frame::read(&mut BufReader::new(reader)).await
            });
            write?;
            let frame = read?.ok_or("frame absent")?;
            assert_eq!([frame.line, frame.body].concat(), input);
        }
        for input in [
            b"PUB a 999999999\r\n".as_slice(),
            b"HPUB a 4 3\r\n",
            b"PUB a 1\r\nxXX",
            b"PING\n",
            b"PUB a -1\r\n",
            b"PUB a 2\r\nx",
        ] {
            assert!(Frame::read(&mut BufReader::new(input)).await.is_err());
        }
        assert!(
            Frame::read(&mut BufReader::new(vec![b'x'; 4097].as_slice()))
                .await
                .is_err()
        );
        Ok(())
    }

    #[test]
    fn only_the_selected_ack_direction_is_dropped() -> TestResult {
        for loss in [Loss::Request, Loss::Confirmation] {
            let state = Mutex::new(State {
                loss,
                reply: None,
                evidence: Evidence::default(),
            });
            let ack = Frame {
                line: b"PUB $JS.ACK.s.c.1.1.1.1.0 _INBOX.research.123 4\r\n".to_vec(),
                body: b"+ACK\r\n".to_vec(),
            };
            assert_eq!(client_drop(&ack, &state)?, matches!(loss, Loss::Request));
            let unrelated = Frame {
                line: b"MSG _INBOX.other 1 0\r\n".to_vec(),
                body: b"\r\n".to_vec(),
            };
            assert!(!server_drop(&unrelated, &state)?);
            let reply = Frame {
                line: b"MSG _INBOX.research.123 1 0\r\n".to_vec(),
                body: b"\r\n".to_vec(),
            };
            assert_eq!(
                server_drop(&reply, &state)?,
                matches!(loss, Loss::Confirmation)
            );
        }
        Ok(())
    }
}
