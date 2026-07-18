use std::io;

use rmcp::model::RequestId;
use tokio::io::{AsyncWrite, AsyncWriteExt};

use super::{ConnectionEnd, MAX_PENDING_REQUESTS};

pub(super) struct Outbox {
    sender: tokio::sync::mpsc::Sender<Vec<u8>>,
    connection_end: ConnectionEnd,
}

impl Clone for Outbox {
    fn clone(&self) -> Self {
        Self {
            sender: self.sender.clone(),
            connection_end: self.connection_end.clone(),
        }
    }
}

impl Outbox {
    pub(super) fn new<W>(
        writer: W,
        connection_end: ConnectionEnd,
    ) -> (Self, tokio::task::JoinHandle<()>)
    where
        W: AsyncWrite + Send + Unpin + 'static,
    {
        let (sender, receiver) = tokio::sync::mpsc::channel(MAX_PENDING_REQUESTS);
        let task_end = connection_end.clone();
        let task = tokio::spawn(write_messages(writer, receiver, task_end));
        (
            Self {
                sender,
                connection_end,
            },
            task,
        )
    }

    pub(super) fn try_send(
        &self,
        bytes: Vec<u8>,
        response_id: Option<&RequestId>,
    ) -> io::Result<()> {
        self.sender.try_send(bytes).map_err(|error| {
            self.connection_end.fail();
            let kind = match error {
                tokio::sync::mpsc::error::TrySendError::Full(_) => io::ErrorKind::WouldBlock,
                tokio::sync::mpsc::error::TrySendError::Closed(_) => io::ErrorKind::BrokenPipe,
            };
            let context = response_id.map_or("protocol output", |_| "protocol response");
            io::Error::new(kind, format!("{context} outbox unavailable"))
        })
    }
}

async fn write_messages<W>(
    mut writer: W,
    mut receiver: tokio::sync::mpsc::Receiver<Vec<u8>>,
    connection_end: ConnectionEnd,
) where
    W: AsyncWrite + Unpin,
{
    while let Some(bytes) = receiver.recv().await {
        if writer.write_all(&bytes).await.is_err()
            || writer.write_all(b"\n").await.is_err()
            || writer.flush().await.is_err()
        {
            connection_end.fail();
            return;
        }
    }
    let _ = writer.shutdown().await;
}
