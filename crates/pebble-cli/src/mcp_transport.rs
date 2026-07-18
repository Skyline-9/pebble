//! Hard-bounded JSON-lines transport for RMCP server messages.

use std::io;
use std::sync::{Arc, atomic::AtomicBool};

use rmcp::model::{ClientJsonRpcMessage, ClientNotification, JsonRpcMessage, RequestId};
use rmcp::service::{RoleServer, TxJsonRpcMessage};
use rmcp::transport::Transport;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

mod admission;
mod frame;
mod outbox;
mod reader;
use admission::{Admission, CancellationStatus, ConnectionEnd};
use frame::FrameDecoder;
use outbox::Outbox;
use reader::{ReaderChannels, read_messages};

pub const MAX_FRAME_BYTES: usize = 1024 * 1024;
pub const MAX_PENDING_REQUESTS: usize = 16;

pub struct Inbound {
    message: ClientJsonRpcMessage,
    permit: Option<OwnedSemaphorePermit>,
}

pub struct BoundedTransport<W> {
    _writer: std::marker::PhantomData<W>,
    outbox: Outbox,
    receiver: tokio::sync::mpsc::Receiver<Inbound>,
    priority_receiver: tokio::sync::mpsc::Receiver<Inbound>,
    deferred_priority: Option<Inbound>,
    pending: Vec<(RequestId, OwnedSemaphorePermit)>,
    #[cfg(test)]
    notification_permits: Arc<Semaphore>,
    admission: Admission,
    reader_task: tokio::task::JoinHandle<()>,
    writer_task: tokio::task::JoinHandle<()>,
}

impl<W> BoundedTransport<W> {
    #[allow(
        clippy::significant_drop_tightening,
        reason = "both admission clones intentionally live for the connection lifetime"
    )]
    pub fn new(
        reader: impl AsyncRead + Send + Unpin + 'static,
        writer: W,
        oversized: Arc<AtomicBool>,
    ) -> (Self, ConnectionEnd)
    where
        W: AsyncWrite + Send + Unpin + 'static,
    {
        let admission = Admission::new();
        let reader_admission = admission.clone();
        let connection_end = ConnectionEnd::new();
        let (outbox, writer_task) = Outbox::new(writer, connection_end.clone());
        let (sender, receiver) = tokio::sync::mpsc::channel(MAX_PENDING_REQUESTS * 2);
        let (priority_sender, priority_receiver) = tokio::sync::mpsc::channel(MAX_PENDING_REQUESTS);
        let notification_permits = Arc::new(Semaphore::new(MAX_PENDING_REQUESTS));
        #[cfg(test)]
        let test_notification_permits = notification_permits.clone();
        let reader_task = tokio::spawn(read_messages(
            FrameDecoder::new(reader, oversized),
            outbox.clone(),
            reader_admission,
            ReaderChannels::new(sender, priority_sender, notification_permits),
            connection_end.clone(),
        ));
        (
            Self {
                _writer: std::marker::PhantomData,
                outbox,
                receiver,
                priority_receiver,
                deferred_priority: None,
                pending: Vec::with_capacity(MAX_PENDING_REQUESTS),
                #[cfg(test)]
                notification_permits: test_notification_permits,
                admission,
                reader_task,
                writer_task,
            },
            connection_end,
        )
    }

    fn take_permit(&mut self, id: &RequestId) -> Option<OwnedSemaphorePermit> {
        let index = self.pending.iter().position(|(pending, _)| pending == id)?;
        Some(self.pending.swap_remove(index).1)
    }

    #[cfg(test)]
    fn admitted_cancellations(&self) -> usize {
        self.admission.cancellation_count()
    }

    #[cfg(test)]
    fn admitted_connection_notifications(&self) -> usize {
        MAX_PENDING_REQUESTS - self.notification_permits.available_permits()
    }

    fn accept(&mut self, inbound: Inbound) -> ClientJsonRpcMessage {
        if let Some(permit) = inbound.permit
            && let JsonRpcMessage::Request(request) = &inbound.message
        {
            self.admission.mark_delivered(&request.id);
            self.pending.push((request.id.clone(), permit));
        }
        inbound.message
    }
}

impl<W> Transport<RoleServer> for BoundedTransport<W>
where
    W: AsyncWrite + Send + Unpin + 'static,
{
    type Error = io::Error;

    fn send(
        &mut self,
        item: TxJsonRpcMessage<RoleServer>,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send + 'static {
        let response_id = response_id(&item).cloned();
        let permit = response_id.as_ref().and_then(|id| self.take_permit(id));
        let outbox = self.outbox.clone();
        let admission = self.admission.clone();
        async move {
            let _permit = permit;
            let result = serde_json::to_vec(&item)
                .map_err(io::Error::other)
                .and_then(|bytes| outbox.try_send(bytes, response_id.as_ref()));
            if let Some(id) = response_id.as_ref() {
                admission.complete(id);
            }
            result
        }
    }

    async fn receive(&mut self) -> Option<ClientJsonRpcMessage> {
        loop {
            if self.deferred_priority.is_none() {
                self.deferred_priority = self.priority_receiver.try_recv().ok();
            }
            match self
                .deferred_priority
                .as_ref()
                .map(|inbound| cancellation_status(inbound, &self.admission))
            {
                Some(CancellationStatus::Ready) => {
                    let inbound = self.deferred_priority.take()?;
                    return Some(self.accept(inbound));
                }
                Some(CancellationStatus::Stale) => {
                    self.deferred_priority.take();
                    continue;
                }
                Some(CancellationStatus::Deferred) => {
                    return self
                        .receiver
                        .recv()
                        .await
                        .map(|inbound| self.accept(inbound));
                }
                None => {}
            }
            tokio::select! {
                biased;
                inbound = self.priority_receiver.recv() => {
                    match inbound {
                        Some(inbound) => match cancellation_status(&inbound, &self.admission) {
                            CancellationStatus::Ready => return Some(self.accept(inbound)),
                            CancellationStatus::Deferred => {
                                self.deferred_priority = Some(inbound);
                            }
                            CancellationStatus::Stale => {}
                        }
                        None => {
                            return self.receiver.recv().await.map(|inbound| self.accept(inbound));
                        }
                    }
                }
                inbound = self.receiver.recv() => {
                    match inbound {
                        Some(inbound) => return Some(self.accept(inbound)),
                        None => {
                            return self.priority_receiver.recv().await
                                .map(|inbound| self.accept(inbound));
                        }
                    }
                }
            }
        }
    }

    async fn close(&mut self) -> Result<(), Self::Error> {
        self.reader_task.abort();
        self.writer_task.abort();
        self.pending.clear();
        self.admission.clear();
        Ok(())
    }
}

fn cancellation_status(inbound: &Inbound, admission: &Admission) -> CancellationStatus {
    let JsonRpcMessage::Notification(notification) = &inbound.message else {
        return CancellationStatus::Ready;
    };
    let ClientNotification::CancelledNotification(cancelled) = &notification.notification else {
        return CancellationStatus::Ready;
    };
    admission.cancellation_status(&cancelled.params.request_id)
}

const fn response_id(message: &TxJsonRpcMessage<RoleServer>) -> Option<&RequestId> {
    match message {
        JsonRpcMessage::Response(response) => Some(&response.id),
        JsonRpcMessage::Error(error) => error.id.as_ref(),
        _ => None,
    }
}

#[cfg(test)]
mod lifecycle_tests;
#[cfg(test)]
mod tests;
