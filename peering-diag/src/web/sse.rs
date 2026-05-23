//! Helpers pour le streaming SSE des jobs.
//!
//! Stratégie sans polling :
//!   1. Snapshot du buffer + subscribe au broadcast sous read-lock (atomique) :
//!      push_line() ne peut pas insérer de ligne pendant ce bloc (besoin du write-lock).
//!   2. Replay du snapshot vers le client.
//!   3. tokio::select! entre rx.recv() (broadcast, ligne par ligne en temps réel)
//!      et done_rx.changed() (watch, signal de fin du job).
//!   4. Sur done : drain du broadcast résiduel + event SSE "done".

use axum::response::sse::{Event, KeepAlive, Sse};
use futures::stream::Stream;
use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::broadcast::error::{RecvError, TryRecvError};

use crate::web::jobs::Job;

/// Crée un stream SSE pour un job : rejoue les lignes déjà produites
/// puis continue en temps réel jusqu'à la fin du job.
pub fn job_stream(
    job: Arc<Job>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let stream = async_stream::stream! {
        // ── 1. Snapshot + subscribe atomiques ────────────────────────────────
        // Tenir le read-lock pendant le snapshot ET la subscription garantit qu'aucune
        // ligne n'est insérée entre les deux : push_line() bloque sur le write-lock.
        // Résultat : les lignes du snapshot ne sont pas dans rx, les suivantes le sont.
        let mut rx = {
            let lines = job.lines.read().await;
            for line in lines.iter() {
                yield Ok(Event::default().data(line.clone()));
            }
            job.tx.subscribe()
            // lock libéré ici
        };

        // ── 2. Subscribe au signal de fin ─────────────────────────────────────
        let mut done_rx = job.done.subscribe();

        // ── 3. Si le job est déjà terminé au moment de la connexion ──────────
        if *done_rx.borrow() {
            loop {
                match rx.try_recv() {
                    Ok(line)                      => yield Ok(Event::default().data(line)),
                    Err(TryRecvError::Lagged(_))  => continue,
                    Err(_)                        => break,
                }
            }
            yield Ok(Event::default().event("done").data(""));
            return;
        }

        // ── 4. Boucle temps réel ──────────────────────────────────────────────
        loop {
            tokio::select! {
                result = rx.recv() => {
                    match result {
                        Ok(line) => yield Ok(Event::default().data(line)),
                        Err(RecvError::Lagged(_)) => {
                            // Très peu probable (buffer=256) mais non bloquant : on continue.
                            continue;
                        }
                        Err(RecvError::Closed) => {
                            yield Ok(Event::default().event("done").data(""));
                            break;
                        }
                    }
                }
                _ = done_rx.changed() => {
                    // Drain les dernières lignes encore dans le canal broadcast
                    loop {
                        match rx.try_recv() {
                            Ok(line)                      => yield Ok(Event::default().data(line)),
                            Err(TryRecvError::Lagged(_))  => continue,
                            Err(_)                        => break,
                        }
                    }
                    yield Ok(Event::default().event("done").data(""));
                    break;
                }
            }
        }
    };

    Sse::new(stream).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("ping"),
    )
}
