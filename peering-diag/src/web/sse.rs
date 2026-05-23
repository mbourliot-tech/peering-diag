//! Helpers pour le streaming SSE des jobs.

use axum::response::sse::{Event, KeepAlive, Sse};
use futures::stream::Stream;
use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;

use crate::web::jobs::Job;

/// Crée un stream SSE pour un job : rejoue les lignes déjà produites
/// puis continue en temps réel jusqu'à la fin du job.
pub fn job_stream(
    job: Arc<Job>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let stream = async_stream::stream! {
        let mut offset = 0usize;

        loop {
            // Rejouer les lignes depuis l'offset courant
            {
                let lines = job.lines.read().await;
                while offset < lines.len() {
                    let event = Event::default().data(lines[offset].clone());
                    offset += 1;
                    yield Ok(event);
                }
            }

            if job.is_done().await {
                // Émettre un event de fin pour que le client puisse fermer la connexion
                yield Ok(Event::default().event("done").data(""));
                break;
            }

            sleep(Duration::from_millis(150)).await;
        }
    };

    Sse::new(stream).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("ping"),
    )
}
