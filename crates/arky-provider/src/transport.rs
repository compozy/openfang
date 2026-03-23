//! Buffered stdio transport with line framing.

use tokio::{
    io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader, BufWriter},
    sync::mpsc,
    task::JoinHandle,
};
use tokio_util::sync::CancellationToken;

use crate::ProviderError;

/// Configuration for [`StdioTransport`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StdioTransportConfig {
    /// Maximum buffered frames waiting to be written.
    pub write_capacity: usize,
    /// Maximum buffered frames waiting to be consumed.
    pub read_capacity: usize,
    /// Maximum allowed frame length in bytes.
    pub max_frame_len: usize,
}

impl Default for StdioTransportConfig {
    fn default() -> Self {
        Self {
            write_capacity: 32,
            read_capacity: 32,
            max_frame_len: 64 * 1024,
        }
    }
}

/// Line-framed buffered transport for subprocess stdin/stdout.
pub struct StdioTransport {
    write_tx: mpsc::Sender<String>,
    read_rx: mpsc::Receiver<Result<String, ProviderError>>,
    read_task: JoinHandle<()>,
    write_task: JoinHandle<()>,
}

impl StdioTransport {
    /// Creates a transport from an async reader and writer pair.
    #[must_use]
    pub fn new<R, W>(reader: R, writer: W, config: StdioTransportConfig) -> Self
    where
        R: AsyncRead + Unpin + Send + 'static,
        W: AsyncWrite + Unpin + Send + 'static,
    {
        let (write_tx, write_rx) = mpsc::channel(config.write_capacity);
        let (read_tx, read_rx) = mpsc::channel(config.read_capacity);

        let read_task = tokio::spawn(read_loop(reader, read_tx, config.max_frame_len));
        let write_task = tokio::spawn(write_loop(writer, write_rx));

        Self {
            write_tx,
            read_rx,
            read_task,
            write_task,
        }
    }

    /// Sends one framed line to the subprocess.
    pub async fn send_frame(
        &self,
        frame: impl Into<String>,
        cancel: CancellationToken,
    ) -> Result<(), ProviderError> {
        let frame = frame.into();
        if frame.contains('\n') || frame.contains('\r') {
            return Err(ProviderError::protocol_violation(
                "frame contains an embedded newline",
                None,
            ));
        }

        tokio::select! {
            () = cancel.cancelled() => Err(ProviderError::stream_interrupted("frame send cancelled")),
            result = self.write_tx.send(frame) => {
                result.map_err(|_| ProviderError::stream_interrupted("stdio writer has shut down"))
            }
        }
    }

    /// Receives the next framed line from the subprocess.
    pub async fn recv_frame(
        &mut self,
        cancel: CancellationToken,
    ) -> Result<Option<String>, ProviderError> {
        tokio::select! {
            () = cancel.cancelled() => Err(ProviderError::stream_interrupted("frame receive cancelled")),
            result = self.read_rx.recv() => {
                match result {
                    Some(Ok(frame)) => Ok(Some(frame)),
                    Some(Err(error)) => Err(error),
                    None => Ok(None),
                }
            }
        }
    }
}

impl Drop for StdioTransport {
    fn drop(&mut self) {
        self.read_task.abort();
        self.write_task.abort();
    }
}

async fn read_loop<R>(
    reader: R,
    read_tx: mpsc::Sender<Result<String, ProviderError>>,
    max_frame_len: usize,
) where
    R: AsyncRead + Unpin,
{
    let mut reader = BufReader::new(reader);
    let mut line = String::new();

    loop {
        line.clear();
        match reader.read_line(&mut line).await {
            Ok(0) => break,
            Ok(_) => {
                trim_line_endings(&mut line);
                if line.len() > max_frame_len {
                    let _ = read_tx
                        .send(Err(ProviderError::protocol_violation(
                            "received frame exceeds configured maximum length",
                            Some(serde_json::json!({
                                "max_frame_len": max_frame_len,
                                "actual_len": line.len(),
                            })),
                        )))
                        .await;
                    break;
                }

                if read_tx.send(Ok(line.clone())).await.is_err() {
                    break;
                }
            }
            Err(error) => {
                let _ = read_tx
                    .send(Err(ProviderError::stream_interrupted(format!(
                        "failed to read from stdio transport: {error}"
                    ))))
                    .await;
                break;
            }
        }
    }
}

async fn write_loop<W>(writer: W, mut write_rx: mpsc::Receiver<String>)
where
    W: AsyncWrite + Unpin,
{
    let mut writer = BufWriter::new(writer);

    while let Some(frame) = write_rx.recv().await {
        if writer.write_all(frame.as_bytes()).await.is_err() {
            break;
        }
        if writer.write_all(b"\n").await.is_err() {
            break;
        }
        if writer.flush().await.is_err() {
            break;
        }
    }

    let _ = writer.flush().await;
}

fn trim_line_endings(line: &mut String) {
    while matches!(line.chars().last(), Some('\n' | '\r')) {
        line.pop();
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use pretty_assertions::assert_eq;
    use tokio::{
        io::{duplex, AsyncBufReadExt, AsyncWriteExt},
        time::sleep,
    };
    use tokio_util::sync::CancellationToken;

    use super::{StdioTransport, StdioTransportConfig};
    use crate::ProviderError;

    #[tokio::test]
    async fn stdio_transport_should_round_trip_framed_messages() {
        let (client_stream, server_stream) = duplex(1_024);
        let (client_read, client_write) = tokio::io::split(client_stream);
        let (server_read, server_write) = tokio::io::split(server_stream);
        let mut transport =
            StdioTransport::new(client_read, client_write, StdioTransportConfig::default());

        tokio::spawn(async move {
            let mut server_reader = tokio::io::BufReader::new(server_read);
            let mut line = String::new();
            server_reader
                .read_line(&mut line)
                .await
                .expect("line should read");
            let mut server_writer = tokio::io::BufWriter::new(server_write);
            server_writer
                .write_all(line.as_bytes())
                .await
                .expect("line should write");
            server_writer.flush().await.expect("writer should flush");
        });

        transport
            .send_frame("ping", CancellationToken::new())
            .await
            .expect("send should succeed");

        let received = transport
            .recv_frame(CancellationToken::new())
            .await
            .expect("receive should succeed")
            .expect("frame should exist");

        assert_eq!(received, "ping");
    }

    #[tokio::test]
    async fn stdio_transport_should_report_cancellation_during_receive() {
        let (reader, writer) = duplex(64);
        let mut transport = StdioTransport::new(reader, writer, StdioTransportConfig::default());
        let cancel = CancellationToken::new();
        cancel.cancel();

        let error = transport
            .recv_frame(cancel)
            .await
            .expect_err("cancelled receive should fail");

        assert!(matches!(error, ProviderError::StreamInterrupted { .. }));
    }

    #[tokio::test]
    async fn stdio_transport_should_apply_backpressure_to_writes() {
        let (client_side, _server_side) = duplex(8);
        let transport = StdioTransport::new(
            tokio::io::empty(),
            client_side,
            StdioTransportConfig {
                write_capacity: 1,
                read_capacity: 1,
                max_frame_len: 8_192,
            },
        );

        transport
            .send_frame("x".repeat(1_024), CancellationToken::new())
            .await
            .expect("first write should enqueue");
        transport
            .send_frame("y".repeat(1_024), CancellationToken::new())
            .await
            .expect("second write should enqueue");

        let cancel = CancellationToken::new();
        let cancelled = cancel.clone();
        tokio::spawn(async move {
            sleep(Duration::from_millis(20)).await;
            cancelled.cancel();
        });

        let error = transport
            .send_frame("z".repeat(1_024), cancel)
            .await
            .expect_err("third write should block until cancellation");

        assert!(matches!(error, ProviderError::StreamInterrupted { .. }));
    }
}
