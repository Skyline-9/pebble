use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;

use rmcp::model::{ErrorData, JsonRpcMessage};
use rmcp::transport::Transport;
use tokio::io::{AsyncWrite, AsyncWriteExt};

use super::*;
use tests::Output;

struct BlockedOutput;

impl AsyncWrite for BlockedOutput {
    fn poll_write(
        self: Pin<&mut Self>,
        _context: &mut Context<'_>,
        _bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        Poll::Pending
    }

    fn poll_flush(self: Pin<&mut Self>, _context: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Pending
    }

    fn poll_shutdown(self: Pin<&mut Self>, _context: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

#[tokio::test]
async fn cancellation_has_priority_over_saturated_ordinary_queue()
-> Result<(), Box<dyn std::error::Error>> {
    let mut input = Vec::new();
    for id in 1..=MAX_PENDING_REQUESTS {
        append(
            &mut input,
            &serde_json::json!({"jsonrpc":"2.0","id":id,"method":"ping"}),
        )?;
    }
    for sequence in 0..MAX_PENDING_REQUESTS {
        append(
            &mut input,
            &serde_json::json!({
                "jsonrpc":"2.0",
                "method":"notifications/unknown",
                "params":{"sequence":sequence}
            }),
        )?;
    }
    append(
        &mut input,
        &serde_json::json!({
            "jsonrpc":"2.0",
            "method":"notifications/cancelled",
            "params":{"requestId":1,"reason":"saturated"}
        }),
    )?;
    let (mut transport, _) = BoundedTransport::new(
        tests::Input::new(input),
        Output::default(),
        Arc::new(AtomicBool::new(false)),
    );
    tokio::task::yield_now().await;
    assert!(matches!(
        transport.receive().await,
        Some(JsonRpcMessage::Request(_))
    ));
    let message = transport.receive().await.ok_or("missing cancellation")?;
    let JsonRpcMessage::Notification(notification) = message else {
        return Err("cancellation did not use priority admission".into());
    };
    let ClientNotification::CancelledNotification(cancelled) = notification.notification else {
        return Err("cancellation did not use priority admission".into());
    };
    assert_eq!(cancelled.params.request_id, RequestId::Number(1));
    assert_eq!(transport.admitted_cancellations(), 1);
    Ok(())
}

#[tokio::test]
async fn stale_deferred_cancellation_does_not_block_later_cancellation()
-> Result<(), Box<dyn std::error::Error>> {
    let (mut input, reader) = tokio::io::duplex(MAX_FRAME_BYTES);
    let (mut transport, _) =
        BoundedTransport::new(reader, Output::default(), Arc::new(AtomicBool::new(false)));
    input
        .write_all(
            b"{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"ping\"}\n\
              {\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"ping\"}\n\
              {\"jsonrpc\":\"2.0\",\"method\":\"notifications/cancelled\",\
              \"params\":{\"requestId\":1}}\n",
        )
        .await?;

    let first = transport.receive().await.ok_or("missing first request")?;
    let JsonRpcMessage::Request(first) = first else {
        return Err("expected first request".into());
    };
    assert_eq!(first.id, RequestId::Number(2));
    wait_for_cancellations(&transport, 1).await?;

    let second = transport.receive().await.ok_or("missing second request")?;
    let JsonRpcMessage::Request(second) = second else {
        return Err("expected second request".into());
    };
    assert_eq!(second.id, RequestId::Number(1));
    input
        .write_all(
            b"{\"jsonrpc\":\"2.0\",\"method\":\"notifications/cancelled\",\
              \"params\":{\"requestId\":2}}\n",
        )
        .await?;
    wait_for_cancellations(&transport, 2).await?;

    transport
        .send(JsonRpcMessage::error(
            ErrorData::internal_error("test", None),
            Some(RequestId::Number(1)),
        ))
        .await?;
    assert_eq!(transport.admitted_cancellations(), 1);

    let cancellation = tokio::time::timeout(Duration::from_millis(100), transport.receive())
        .await?
        .ok_or("missing later cancellation")?;
    let JsonRpcMessage::Notification(cancellation) = cancellation else {
        return Err("expected later cancellation".into());
    };
    let ClientNotification::CancelledNotification(cancellation) = cancellation.notification else {
        return Err("expected later cancellation".into());
    };
    assert_eq!(cancellation.params.request_id, RequestId::Number(2));
    assert_eq!(transport.admitted_cancellations(), 1);
    Ok(())
}

#[tokio::test]
async fn delivered_notifications_release_slots_and_initialized_is_once()
-> Result<(), Box<dyn std::error::Error>> {
    let (mut input, reader) = tokio::io::duplex(MAX_FRAME_BYTES);
    let (mut transport, _) =
        BoundedTransport::new(reader, Output::default(), Arc::new(AtomicBool::new(false)));
    write_notifications(&mut input, 0).await?;
    for _ in 0..MAX_PENDING_REQUESTS {
        assert!(matches!(
            transport.receive().await,
            Some(JsonRpcMessage::Notification(_))
        ));
    }
    assert_eq!(transport.admitted_connection_notifications(), 0);

    write_notifications(&mut input, MAX_PENDING_REQUESTS).await?;
    for _ in 0..MAX_PENDING_REQUESTS {
        assert!(matches!(
            transport.receive().await,
            Some(JsonRpcMessage::Notification(_))
        ));
    }
    input
        .write_all(
            b"{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}\n\
              {\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}\n",
        )
        .await?;
    let initialized = transport.receive().await.ok_or("missing initialized")?;
    let JsonRpcMessage::Notification(initialized) = initialized else {
        return Err("expected initialized notification".into());
    };
    assert!(matches!(
        initialized.notification,
        ClientNotification::InitializedNotification(_)
    ));
    assert!(
        tokio::time::timeout(Duration::from_millis(20), transport.receive())
            .await
            .is_err()
    );
    Ok(())
}

#[tokio::test]
async fn blocked_stdout_cannot_hide_reader_eof() -> Result<(), Box<dyn std::error::Error>> {
    let input = tests::Input::new(b"{not json}\n".to_vec());
    let (_transport, connection_end) =
        BoundedTransport::new(input, BlockedOutput, Arc::new(AtomicBool::new(false)));
    tokio::time::timeout(Duration::from_millis(100), connection_end.wait())
        .await
        .map_err(|_| "reader awaited blocked stdout after EOF")?;
    Ok(())
}

#[tokio::test]
async fn saturated_outbox_fails_connection() -> Result<(), Box<dyn std::error::Error>> {
    let (mut input, reader) = tokio::io::duplex(64);
    let (mut transport, connection_end) =
        BoundedTransport::new(reader, BlockedOutput, Arc::new(AtomicBool::new(false)));
    input.write_all(b"\n").await?;
    tokio::task::yield_now().await;
    let mut rejected = false;
    for id in 0..MAX_PENDING_REQUESTS + 2 {
        let message = JsonRpcMessage::error(
            ErrorData::internal_error("test", None),
            Some(RequestId::Number(i64::try_from(id)?)),
        );
        if transport.send(message).await.is_err() {
            rejected = true;
            break;
        }
    }
    assert!(rejected, "bounded outbox accepted unbounded writes");
    tokio::time::timeout(Duration::from_millis(100), connection_end.wait()).await?;
    assert!(connection_end.failed());
    Ok(())
}

#[tokio::test]
async fn saturated_cancellation_path_fails_connection() -> Result<(), Box<dyn std::error::Error>> {
    let mut input = Vec::new();
    for id in 1..=MAX_PENDING_REQUESTS {
        append(
            &mut input,
            &serde_json::json!({"jsonrpc":"2.0","id":id,"method":"ping"}),
        )?;
    }
    append(
        &mut input,
        &serde_json::json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
    )?;
    for id in 1..=MAX_PENDING_REQUESTS {
        append(
            &mut input,
            &serde_json::json!({
                "jsonrpc":"2.0",
                "method":"notifications/cancelled",
                "params":{"requestId":id}
            }),
        )?;
    }
    let (_transport, connection_end) = BoundedTransport::new(
        tests::Input::new(input),
        Output::default(),
        Arc::new(AtomicBool::new(false)),
    );
    tokio::time::timeout(Duration::from_millis(100), connection_end.wait()).await?;
    assert!(connection_end.failed());
    Ok(())
}

async fn write_notifications(
    input: &mut tokio::io::DuplexStream,
    start: usize,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut bytes = Vec::new();
    for sequence in start..start + MAX_PENDING_REQUESTS {
        append(
            &mut bytes,
            &serde_json::json!({
                "jsonrpc":"2.0",
                "method":"notifications/unknown",
                "params":{"sequence":sequence}
            }),
        )?;
    }
    input.write_all(&bytes).await?;
    Ok(())
}

async fn wait_for_cancellations(
    transport: &BoundedTransport<Output>,
    expected: usize,
) -> Result<(), Box<dyn std::error::Error>> {
    tokio::time::timeout(Duration::from_millis(100), async {
        while transport.admitted_cancellations() != expected {
            tokio::task::yield_now().await;
        }
    })
    .await?;
    Ok(())
}

fn append(
    bytes: &mut Vec<u8>,
    message: &serde_json::Value,
) -> Result<(), Box<dyn std::error::Error>> {
    serde_json::to_writer(&mut *bytes, message)?;
    bytes.push(b'\n');
    Ok(())
}
