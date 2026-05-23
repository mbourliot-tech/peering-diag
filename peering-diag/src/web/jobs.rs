//! Gestion des jobs longue durée (diag, mtr, watch…).
//!
//! Chaque job spawne le binaire courant en sous-processus, capture stdout+stderr
//! ligne par ligne, et bufferise les lignes pour les rejouer aux clients SSE
//! qui se connectent après le démarrage.

use anyhow::{anyhow, Context, Result};
use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::sync::{broadcast, watch, RwLock, Semaphore};
use uuid::Uuid;

const MAX_CONCURRENT_JOBS: usize = 8;
pub const JOB_TIMEOUT_SECS: u64  = 600; // 10 min — pour les jobs standards (non-watch)
const LINE_BUFFER_CAP:     usize = 5_000;

// ─── État d'un job ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum JobStatus {
    Running,
    Done,
    Failed,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct JobInfo {
    pub id:      Uuid,
    pub command: String,
    pub status:  JobStatus,
}

pub struct Job {
    pub id:      Uuid,
    pub command: String,
    // Lignes déjà produites (pour les clients qui se connectent en retard).
    // Bornée à LINE_BUFFER_CAP : les plus anciennes sont droppées en tête.
    pub lines:   Arc<RwLock<VecDeque<String>>>,
    // Canal broadcast pour les lignes nouvelles (vrai temps réel)
    pub tx:      broadcast::Sender<String>,
    pub status:  Arc<RwLock<JobStatus>>,
    // Signal de fin : watch::Sender<bool> — envoie true quand le job se termine.
    // Les clients SSE s'abonnent via job.done.subscribe() pour éviter le polling.
    pub done:    watch::Sender<bool>,
    // Handle d'annulation de la task qui supervise le subprocess.
    // L'abandon de la task libère le `Child` (kill_on_drop) → le process est tué.
    pub abort:   RwLock<Option<tokio::task::AbortHandle>>,
}

impl Job {
    pub fn new(id: Uuid, command: impl Into<String>) -> Self {
        let (tx, _)   = broadcast::channel(256);
        let (done, _) = watch::channel(false);
        Self {
            id,
            command: command.into(),
            lines:   Arc::new(RwLock::new(VecDeque::new())),
            tx,
            status:  Arc::new(RwLock::new(JobStatus::Running)),
            done,
            abort:   RwLock::new(None),
        }
    }

    /// Tue le subprocess associé : abandonne la task de supervision, ce qui
    /// libère le `Child` (configuré avec `kill_on_drop`) et marque le job terminé.
    pub async fn kill(&self) {
        if let Some(handle) = self.abort.write().await.take() {
            handle.abort();
        }
        *self.status.write().await = JobStatus::Done;
    }

    pub async fn info(&self) -> JobInfo {
        JobInfo {
            id:      self.id,
            command: self.command.clone(),
            status:  self.status.read().await.clone(),
        }
    }

    pub async fn is_done(&self) -> bool {
        *self.status.read().await != JobStatus::Running
    }
}

// ─── Manager ─────────────────────────────────────────────────────────────────

pub struct JobManager {
    jobs:      RwLock<HashMap<Uuid, Arc<Job>>>,
    semaphore: Arc<Semaphore>,
}

impl JobManager {
    pub fn new() -> Self {
        Self {
            jobs:      RwLock::new(HashMap::new()),
            semaphore: Arc::new(Semaphore::new(MAX_CONCURRENT_JOBS)),
        }
    }

    pub async fn get(&self, id: Uuid) -> Option<Arc<Job>> {
        self.jobs.read().await.get(&id).cloned()
    }

    pub async fn list(&self) -> Vec<JobInfo> {
        let jobs = self.jobs.read().await;
        let mut infos = Vec::new();
        for job in jobs.values() {
            infos.push(job.info().await);
        }
        infos
    }

    /// Spawne un sous-processus avec les args donnés.
    /// `timeout_secs` : Some(n) pour tuer après n secondes, None = pas de timeout (ex: watch).
    /// Retourne l'UUID du job créé, ou une erreur si le plafond de concurrence est atteint.
    pub async fn spawn(
        self: &Arc<Self>,
        command_label: String,
        args: Vec<String>,
        db_path: Option<PathBuf>,
        timeout_secs: Option<u64>,
    ) -> Result<Uuid> {
        // Limiter la concurrence : erreur immédiate si le plafond est atteint.
        let permit = Arc::clone(&self.semaphore)
            .try_acquire_owned()
            .map_err(|_| anyhow!(
                "Trop de jobs simultanés (max {}). Réessayez dans un instant.",
                MAX_CONCURRENT_JOBS
            ))?;

        let id  = Uuid::new_v4();
        let job = Arc::new(Job::new(id, command_label));

        {
            let mut jobs = self.jobs.write().await;
            jobs.insert(id, job.clone());
        }

        // Chemin du binaire courant
        let bin = std::env::current_exe().context("current_exe")?;

        let mut cmd_args = args;
        // Injecter --db si fourni et si pas déjà présent
        if let Some(db) = db_path {
            if !cmd_args.contains(&"--db".to_string()) {
                cmd_args.push("--db".to_string());
                cmd_args.push(db.to_string_lossy().to_string());
            }
        }

        let mut child = Command::new(&bin)
            .args(&cmd_args)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .context("spawn subprocess")?;

        let stdout = child.stdout.take().unwrap();
        let stderr = child.stderr.take().unwrap();

        let job_clone = job.clone();
        let manager   = self.clone();

        // Task : lit stdout + stderr et alimente le buffer + broadcast.
        // Le permit est tenu jusqu'à la fin de la task → slot libéré automatiquement.
        let handle = tokio::spawn(async move {
            let _permit = permit;

            // Cloner job_clone pour le passer dans la future interne (move)
            let job_inner = job_clone.clone();

            let inner_fut = async move {
                let mut out = BufReader::new(stdout).lines();
                let mut err = BufReader::new(stderr).lines();

                loop {
                    tokio::select! {
                        line = out.next_line() => {
                            match line {
                                Ok(Some(l)) => job_inner.push_line(&l).await,
                                _ => break,
                            }
                        }
                        line = err.next_line() => {
                            match line {
                                Ok(Some(l)) => job_inner.push_line(&l).await,
                                _ => {}
                            }
                        }
                    }
                }
                // Vider stderr restant
                while let Ok(Some(l)) = err.next_line().await {
                    job_inner.push_line(&l).await;
                }

                // Attendre la fin du processus
                let status = child.wait().await;
                let final_status = match status {
                    Ok(s) if s.success() => JobStatus::Done,
                    _ => JobStatus::Failed,
                };
                *job_inner.status.write().await = final_status;
                let _ = job_inner.done.send(true);
            };

            let did_timeout = if let Some(secs) = timeout_secs {
                // Le sous-processus est tué via kill_on_drop quand la future est droppée.
                tokio::time::timeout(Duration::from_secs(secs), inner_fut)
                    .await
                    .is_err()
            } else {
                inner_fut.await;
                false
            };

            if did_timeout {
                job_clone
                    .push_line(&format!(
                        "[peering-diag] Job interrompu : timeout de {} min dépassé.",
                        timeout_secs.unwrap_or(0) / 60
                    ))
                    .await;
                *job_clone.status.write().await = JobStatus::Failed;
                let _ = job_clone.done.send(true);
            }

            // Nettoyer les vieux jobs terminés après 10 min
            let id = job_clone.id;
            tokio::time::sleep(Duration::from_secs(600)).await;
            manager.jobs.write().await.remove(&id);
        });

        // Mémoriser le handle pour pouvoir tuer le subprocess via Job::kill().
        *job.abort.write().await = Some(handle.abort_handle());

        Ok(id)
    }

    /// Retire un job du manager (utilisé après un arrêt explicite).
    pub async fn remove(&self, id: Uuid) {
        self.jobs.write().await.remove(&id);
    }
}

impl Job {
    async fn push_line(&self, raw: &str) {
        // Strip des séquences ANSI pour affichage propre dans le navigateur
        let clean = strip_ansi_escapes::strip_str(raw);
        let line  = clean.trim_end().to_string();
        if line.is_empty() { return; }

        let mut lines = self.lines.write().await;
        if lines.len() >= LINE_BUFFER_CAP {
            lines.pop_front();
        }
        lines.push_back(line.clone());
        drop(lines);
        // Ignorer l'erreur si aucun récepteur
        let _ = self.tx.send(line);
    }
}
