//! peering-diag : outil de diagnostic de peering réseau.
//!
//! Combine un MTR raffiné (AS-aware, détection ECMP, détection ICMP rate-limit)
//! avec des speedtests segmentés par AS du chemin, pour localiser les problèmes
//! de peering / d'interconnexion.

pub mod asn;
pub mod lg;
pub mod mtr;
pub mod report;
pub mod speedtest;
pub mod types;
pub mod web;
