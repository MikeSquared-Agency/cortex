use cortex_core::ingest::{IngestAdapter, IngestEvent};
use cortex_core::Result;
use async_trait::async_trait;
use futures::stream::{self, BoxStream, StreamExt};

pub struct StdinAdapter;

#[async_trait]
impl IngestAdapter for StdinAdapter {
    fn name(&self) -> &str {
        "stdin"
    }

    async fn subscribe(&self) -> Result<BoxStream<'static, IngestEvent>> {
        use tokio::io::{AsyncBufReadExt, BufReader};

        let stdin = tokio::io::stdin();
        let reader = BufReader::new(stdin);
        let lines = reader.lines();

        let s = stream::unfold(lines, |mut lines| async move {
            match lines.next_line().await {
                Ok(Some(line)) => {
                    let event = serde_json::from_str::<IngestEvent>(&line).ok();
                    Some((event, lines))
                }
                _ => None,
            }
        })
        .filter_map(|opt| async move { opt });

        Ok(Box::pin(s))
    }
}
