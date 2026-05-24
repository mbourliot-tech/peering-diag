//! Structures de sortie JSON structurée pour `--format json`.

use crate::lg::globalping::{MtrHop, ProbeInfo};
use crate::mtr::tcp_probe::{EcmpImbalance, FlowStats};
use crate::types::{DiagnosticReport, Finding, Verdict};
use serde::Serialize;

/// Sortie JSON complète pour `diag` (aller + retour).
#[derive(Serialize)]
pub struct DiagJson {
    #[serde(flatten)]
    pub aller: DiagnosticReport,
    pub retour: Option<RetourJson>,
}

/// Sortie JSON du chemin retour (Globalping).
#[derive(Serialize)]
pub struct RetourJson {
    pub probe: ProbeInfo,
    pub hops: Vec<MtrHop>,
    pub findings: Vec<Finding>,
    pub verdict: Verdict,
}

/// Sortie JSON pour `ecmp`.
#[derive(Serialize)]
pub struct EcmpJson {
    pub target: String,
    pub dst_port: u16,
    pub flows: Vec<FlowStats>,
    pub imbalance: EcmpImbalance,
}
