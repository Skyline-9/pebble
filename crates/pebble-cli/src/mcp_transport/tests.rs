use std::pin::Pin;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use std::time::Duration;

use rmcp::model::{ErrorCode, ErrorData, JsonRpcMessage};
use rmcp::service::{RoleServer, TxJsonRpcMessage};
use rmcp::transport::Transport;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

use super::*;

pub(super) struct Input {
    bytes: Vec<u8>,
    offset: usize,
}

impl Input {
    pub(super) const fn new(bytes: Vec<u8>) -> Self {
        Self { bytes, offset: 0 }
    }
}

impl AsyncRead for Input {
    fn poll_read(
        mut self: Pin<&mut Self>,
        _context: &mut Context<'_>,
        output: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let remaining = &self.bytes[self.offset..];
        let count = remaining.len().min(output.remaining());
        output.put_slice(&remaining[..count]);
        self.offset += count;
        Poll::Ready(Ok(()))
    }
}

#[derive(Clone, Default)]
pub(super) struct Output(pub(super) Arc<Mutex<Vec<u8>>>);

impl AsyncWrite for Output {
    fn poll_write(
        self: Pin<&mut Self>,
        _context: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<Result<usize, io::Error>> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .extend_from_slice(bytes);
        Poll::Ready(Ok(bytes.len()))
    }

    fn poll_flush(self: Pin<&mut Self>, _context: &mut Context<'_>) -> Poll<Result<(), io::Error>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(
        self: Pin<&mut Self>,
        _context: &mut Context<'_>,
    ) -> Poll<Result<(), io::Error>> {
        Poll::Ready(Ok(()))
    }
}

#[tokio::test]
async fn decoder_accepts_exact_limit_with_fixed_capacity() -> Result<(), Box<dyn std::error::Error>>
{
    let mut bytes = vec![b'x'; MAX_FRAME_BYTES - 1];
    bytes.push(b'\n');
    let mut decoder = FrameDecoder::new(Input::new(bytes), Arc::new(AtomicBool::new(false)));
    let frame = decoder.read_frame().await?.ok_or("missing frame")?;
    assert_eq!(frame.len(), MAX_FRAME_BYTES - 1);
    assert_eq!(frame.capacity(), MAX_FRAME_BYTES);
    assert_eq!(decoder.frame.capacity(), MAX_FRAME_BYTES);
    Ok(())
}

#[tokio::test]
async fn decoder_rejects_limit_plus_one_without_capacity_growth() {
    let oversized = Arc::new(AtomicBool::new(false));
    let mut bytes = vec![b'x'; MAX_FRAME_BYTES];
    bytes.push(b'\n');
    let mut decoder = FrameDecoder::new(Input::new(bytes), oversized.clone());
    let result = decoder.read_frame().await;
    assert!(result.is_err());
    assert!(oversized.load(Ordering::Acquire));
    assert_eq!(decoder.frame.len(), MAX_FRAME_BYTES);
    assert_eq!(decoder.frame.capacity(), MAX_FRAME_BYTES);
}

#[tokio::test]
async fn scheduler_never_admits_more_than_pending_limit() -> Result<(), Box<dyn std::error::Error>>
{
    let mut input = Vec::new();
    for id in 1..=MAX_PENDING_REQUESTS {
        serde_json::to_writer(
            &mut input,
            &serde_json::json!({"jsonrpc":"2.0","id":id,"method":"ping"}),
        )?;
        input.push(b'\n');
    }
    serde_json::to_writer(
        &mut input,
        &serde_json::json!({
            "jsonrpc":"2.0",
            "method":"notifications/cancelled",
            "params":{"requestId":1,"reason":"test"}
        }),
    )?;
    input.push(b'\n');
    serde_json::to_writer(
        &mut input,
        &serde_json::json!({
            "jsonrpc":"2.0",
            "id":MAX_PENDING_REQUESTS + 1,
            "method":"ping"
        }),
    )?;
    input.push(b'\n');
    let output = Output::default();
    let (mut transport, _) = BoundedTransport::new(
        Input::new(input),
        output.clone(),
        Arc::new(AtomicBool::new(false)),
    );
    let first = transport.receive().await.ok_or("missing first request")?;
    let JsonRpcMessage::Request(first_request) = first else {
        return Err("expected first request".into());
    };
    let first_id = first_request.id;
    let cancellation = transport.receive().await.ok_or("missing cancellation")?;
    assert!(matches!(cancellation, JsonRpcMessage::Notification(_)));
    for expected in 2..=MAX_PENDING_REQUESTS {
        let message = transport.receive().await.ok_or("missing request")?;
        let JsonRpcMessage::Request(request) = message else {
            return Err("expected request".into());
        };
        assert_eq!(request.id, RequestId::Number(i64::try_from(expected)?));
    }
    assert_eq!(transport.pending.len(), MAX_PENDING_REQUESTS);
    assert_eq!(transport.pending.capacity(), MAX_PENDING_REQUESTS);
    assert_eq!(transport.admission.active_count(), MAX_PENDING_REQUESTS);
    let blocked = tokio::time::timeout(Duration::from_millis(20), transport.receive()).await;
    assert!(
        !matches!(blocked, Ok(Some(JsonRpcMessage::Request(_)))),
        "seventeenth request was admitted"
    );
    assert_eq!(transport.pending.len(), MAX_PENDING_REQUESTS);

    let response: TxJsonRpcMessage<RoleServer> = JsonRpcMessage::error(
        ErrorData::new(ErrorCode(-32_800), "Request cancelled", None),
        Some(first_id),
    );
    transport.send(response).await?;
    tokio::task::yield_now().await;
    assert_eq!(transport.pending.len(), MAX_PENDING_REQUESTS - 1);
    assert_eq!(transport.admission.active_count(), MAX_PENDING_REQUESTS - 1);
    transport.close().await?;
    drop(transport);
    let written = output
        .0
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone();
    let text = String::from_utf8(written)?;
    assert!(text.contains("too many pending requests"));
    assert!(text.contains("Request cancelled"));
    Ok(())
}

#[tokio::test]
async fn duplicate_cancellations_admit_only_one_until_response()
-> Result<(), Box<dyn std::error::Error>> {
    let mut input = Vec::new();
    serde_json::to_writer(
        &mut input,
        &serde_json::json!({"jsonrpc":"2.0","id":1,"method":"ping"}),
    )?;
    input.push(b'\n');
    for _ in 0..5_000 {
        serde_json::to_writer(
            &mut input,
            &serde_json::json!({
                "jsonrpc":"2.0",
                "method":"notifications/cancelled",
                "params":{"requestId":1,"reason":"duplicate"}
            }),
        )?;
        input.push(b'\n');
    }
    let (mut transport, _) = BoundedTransport::new(
        Input::new(input),
        Output::default(),
        Arc::new(AtomicBool::new(false)),
    );
    assert!(matches!(
        transport.receive().await,
        Some(JsonRpcMessage::Request(_))
    ));
    assert!(matches!(
        transport.receive().await,
        Some(JsonRpcMessage::Notification(_))
    ));
    assert_eq!(transport.admitted_cancellations(), 1);
    assert!(
        tokio::time::timeout(Duration::from_millis(50), transport.receive())
            .await
            .ok()
            .flatten()
            .is_none()
    );
    assert_eq!(transport.admitted_cancellations(), 1);
    transport
        .send(JsonRpcMessage::error(
            ErrorData::new(ErrorCode(-32_800), "Request cancelled", None),
            Some(RequestId::Number(1)),
        ))
        .await?;
    assert_eq!(transport.admitted_cancellations(), 0);
    Ok(())
}

#[tokio::test]
async fn notification_fan_out_stays_bounded_until_close() -> Result<(), Box<dyn std::error::Error>>
{
    let mut input = Vec::new();
    for _ in 0..2_000 {
        input
            .extend_from_slice(b"{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}\n");
        input.extend_from_slice(
            b"{\"jsonrpc\":\"2.0\",\"method\":\"notifications/roots/list_changed\"}\n",
        );
    }
    let (mut transport, _) = BoundedTransport::new(
        Input::new(input),
        Output::default(),
        Arc::new(AtomicBool::new(false)),
    );
    let mut admitted = 0;
    while let Ok(Some(JsonRpcMessage::Notification(_))) =
        tokio::time::timeout(Duration::from_millis(50), transport.receive()).await
    {
        admitted += 1;
    }
    assert_eq!(admitted, MAX_PENDING_REQUESTS + 1);
    assert_eq!(transport.admitted_connection_notifications(), 0);
    transport.close().await?;
    assert_eq!(transport.admitted_connection_notifications(), 0);
    Ok(())
}
