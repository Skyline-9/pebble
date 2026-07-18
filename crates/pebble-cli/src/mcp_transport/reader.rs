use std::io;
use std::sync::Arc;

use rmcp::model::{ClientJsonRpcMessage, ClientNotification, ErrorData, JsonRpcMessage, RequestId};
use rmcp::service::{RoleServer, TxJsonRpcMessage};
use tokio::io::AsyncRead;
use tokio::sync::Semaphore;

use super::{Admission, ConnectionEnd, FrameDecoder, Inbound, Outbox};

pub(super) struct ReaderChannels {
    ordinary: tokio::sync::mpsc::Sender<Inbound>,
    priority: tokio::sync::mpsc::Sender<Inbound>,
    notification_permits: Arc<Semaphore>,
}

impl ReaderChannels {
    pub(super) const fn new(
        ordinary: tokio::sync::mpsc::Sender<Inbound>,
        priority: tokio::sync::mpsc::Sender<Inbound>,
        notification_permits: Arc<Semaphore>,
    ) -> Self {
        Self {
            ordinary,
            priority,
            notification_permits,
        }
    }
}

pub(super) async fn read_messages<R>(
    mut reader: FrameDecoder<R>,
    outbox: Outbox,
    admission: Admission,
    channels: ReaderChannels,
    connection_end: ConnectionEnd,
) where
    R: AsyncRead + Send + Unpin + 'static,
{
    let end_guard = ConnectionEndGuard(connection_end);
    let mut initialized = false;
    while let Ok(Some(frame)) = reader.read_frame().await {
        if frame.is_empty() {
            continue;
        }
        let Ok(message) = serde_json::from_slice::<ClientJsonRpcMessage>(&frame) else {
            let error = TxJsonRpcMessage::<RoleServer>::error(
                ErrorData::parse_error("Parse error", None),
                None,
            );
            if queue_message(&outbox, &error, None).is_err() {
                return;
            }
            continue;
        };
        let permit = match &message {
            JsonRpcMessage::Request(request) => {
                let Some(permit) = admission.try_request(&request.id) else {
                    let error = TxJsonRpcMessage::<RoleServer>::error(
                        ErrorData::new(
                            rmcp::model::ErrorCode(-32_000),
                            "too many pending requests",
                            None,
                        ),
                        Some(request.id.clone()),
                    );
                    if queue_message(&outbox, &error, Some(&request.id)).is_err() {
                        return;
                    }
                    continue;
                };
                Some(permit)
            }
            JsonRpcMessage::Notification(notification) => {
                if let ClientNotification::CancelledNotification(cancelled) =
                    &notification.notification
                {
                    let request_id = cancelled.params.request_id.clone();
                    if !admission.try_cancellation(&request_id) {
                        continue;
                    }
                    if channels
                        .priority
                        .try_send(Inbound {
                            message,
                            permit: None,
                        })
                        .is_err()
                    {
                        end_guard.0.fail();
                        return;
                    }
                    admission.admit_cancellation(&request_id);
                    continue;
                }
                if matches!(
                    notification.notification,
                    ClientNotification::InitializedNotification(_)
                ) {
                    if initialized {
                        continue;
                    }
                    if channels
                        .priority
                        .try_send(Inbound {
                            message,
                            permit: None,
                        })
                        .is_err()
                    {
                        end_guard.0.fail();
                        return;
                    }
                    initialized = true;
                    continue;
                }
                let Ok(permit) = channels.notification_permits.clone().try_acquire_owned() else {
                    continue;
                };
                let _ = channels.ordinary.try_send(Inbound {
                    message,
                    permit: Some(permit),
                });
                continue;
            }
            _ => continue,
        };
        if let Err(error) = channels.ordinary.try_send(Inbound { message, permit }) {
            if let JsonRpcMessage::Request(request) = &error.into_inner().message {
                admission.complete(&request.id);
            }
            end_guard.0.fail();
            return;
        }
    }
}

fn queue_message(
    outbox: &Outbox,
    message: &TxJsonRpcMessage<RoleServer>,
    response_id: Option<&RequestId>,
) -> io::Result<()> {
    let bytes = serde_json::to_vec(message).map_err(io::Error::other)?;
    outbox.try_send(bytes, response_id)
}

struct ConnectionEndGuard(ConnectionEnd);

impl Drop for ConnectionEndGuard {
    fn drop(&mut self) {
        self.0.finish();
    }
}
