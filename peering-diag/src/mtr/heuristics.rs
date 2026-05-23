//! Heuristiques pour distinguer les vraies pertes des artefacts de mesure.
//!
//! Pièges connus :
//! 1. ICMP rate-limiting : un routeur limite ses réponses ICMP mais route bien
//!    le trafic. La perte apparente n'est pas réelle.
//! 2. "Trains" de routeurs rate-limitants : plusieurs hops consécutifs peuvent
//!    tous limiter ICMP. Il faut regarder le PROCHAIN HOP SANS RATE-LIMIT pour
//!    déterminer la vraie perte.
//! 3. Bond de latence physique : un gros saut de RTT peut juste être la
//!    traversée transatlantique, pas une congestion. On ne l'appelle "dégradation"
//!    que si la perte se propage AUSSI.

use crate::types::Hop;

/// Marque les hops suspects d'ICMP rate-limiting.
///
/// Algorithme :
/// - Pour chaque hop avec perte > 5%, on cherche LE PROCHAIN HOP qui a une
///   perte FAIBLE (<5%). Si on en trouve un, alors TOUS les hops entre le
///   courant et celui-ci (exclu) ont une "perte apparente" non réelle —
///   c'est du rate-limiting.
/// - Si on ne trouve pas de hop "propre" derrière, on considère que la perte
///   est réelle.
pub fn flag_icmp_ratelimiting(hops: &mut [Hop]) {
    let n = hops.len();
    if n == 0 {
        return;
    }

    // Pour chaque hop avec perte significative, on cherche un "hop propre" devant
    for i in 0..n {
        let cur_loss = hops[i].loss_pct();
        if cur_loss < 5.0 {
            continue;
        }

        // Cherche le prochain hop avec perte < 5% (et qui a répondu au moins une fois)
        let clean_ahead = hops[(i + 1)..]
            .iter()
            .find(|h| h.received > 0 && h.loss_pct() < 5.0);

        if clean_ahead.is_some() {
            // Il existe un hop "propre" derrière → la perte ici est du rate-limit
            hops[i].suspected_icmp_ratelimit = true;
        }
        // Sinon : pas de hop propre derrière, on garde la perte comme potentiellement réelle
    }
}

/// Détecte les hops avec bufferbloat.
pub fn is_bufferbloated(hop: &Hop) -> bool {
    match (hop.min_rtt_ms(), hop.max_rtt_ms()) {
        (Some(min), Some(max)) if min > 0.0 => {
            let ratio = max / min;
            let absolute_jump = max - min;
            // Ignorer les hops avec RTT min très bas (<10ms) : le moindre spike
            // donne un ratio énorme sans que ça soit du vrai bufferbloat.
            // On exige aussi un écart absolu >80ms pour éviter les faux positifs.
            min >= 10.0 && ratio > 5.0 && absolute_jump > 80.0
        }
        _ => false,
    }
}

/// Type de dégradation identifié.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DegradationType {
    /// Bond de latence pur, sans perte → probablement physique (océan, distance)
    LatencyJump,
    /// Perte de paquets qui se propage → vraie dégradation
    PacketLoss,
    /// Latence ET perte → congestion sérieuse
    Congestion,
}

#[derive(Debug, Clone)]
pub struct Degradation {
    pub hop_index: usize,
    pub degradation_type: DegradationType,
    pub rtt_jump_ms: f64,
    pub loss_pct: f64,
}

/// Identifie le point où la dégradation commence.
///
/// Distingue trois cas :
/// - Bond de RTT >30ms sans perte propagée → probablement physique (transatlantique etc.)
/// - Perte qui se propage sur hops suivants → vraie perte réseau
/// - Les deux → congestion
///
/// Important : on analyse les RTT même sur les hops marqués rate-limit.
/// Le flag rate-limit concerne les RÉPONSES ICMP, pas la fiabilité des RTT.
/// On skip uniquement les hops sans aucun RTT mesuré (timeouts complets).
pub fn find_degradation(hops: &[Hop]) -> Option<Degradation> {
    let n = hops.len();
    if n < 2 {
        return None;
    }

    let mut best: Option<Degradation> = None;

    for i in 1..n {
        let cur = &hops[i];

        // Skip les hops sans aucun RTT (timeouts complets, ex: hop "*")
        if cur.rtt_samples.is_empty() {
            continue;
        }

        // Pour le RTT de référence "avant", on prend le dernier hop qui a des RTT,
        // pas forcément i-1 (qui peut être un timeout complet).
        let prev_rtt = hops[..i]
            .iter()
            .rev()
            .find_map(|h| h.min_rtt_ms());

        // Calcul du bond de RTT (en min, plus stable que avg)
        let rtt_jump = match (prev_rtt, cur.min_rtt_ms()) {
            (Some(p), Some(c)) => c - p,
            _ => 0.0,
        };

        // Pour la perte, on ignore les hops rate-limited (leur perte est apparente)
        let cur_loss = if cur.suspected_icmp_ratelimit {
            0.0
        } else {
            cur.loss_pct()
        };

        // La perte se propage-t-elle sur les hops suivants non-rate-limited ?
        let loss_propagates = cur_loss > 3.0 && {
            let following: Vec<&Hop> = hops[i..]
                .iter()
                .filter(|h| !h.suspected_icmp_ratelimit && h.received > 0)
                .take(3)
                .collect();

            !following.is_empty()
                && following
                    .iter()
                    .all(|h| h.loss_pct() >= cur_loss - 3.0)
        };

        let big_latency_jump = rtt_jump > 30.0;

        if !loss_propagates && !big_latency_jump {
            continue;
        }

        let degradation_type = match (loss_propagates, big_latency_jump) {
            (true, true) => DegradationType::Congestion,
            (true, false) => DegradationType::PacketLoss,
            (false, true) => DegradationType::LatencyJump,
            _ => continue,
        };

        let candidate = Degradation {
            hop_index: i,
            degradation_type,
            rtt_jump_ms: rtt_jump,
            loss_pct: cur_loss,
        };

        // Garde la dégradation la plus sévère (priorité : Congestion > PacketLoss > LatencyJump)
        match (&best, &candidate.degradation_type) {
            (None, _) => best = Some(candidate),
            (Some(b), DegradationType::Congestion)
                if b.degradation_type != DegradationType::Congestion =>
            {
                best = Some(candidate);
            }
            (Some(b), DegradationType::PacketLoss)
                if b.degradation_type == DegradationType::LatencyJump =>
            {
                best = Some(candidate);
            }
            _ => {}
        }
    }

    best
}

/// Conservé pour compatibilité avec l'ancien analyzer.
pub fn find_degradation_point(hops: &[Hop]) -> Option<usize> {
    find_degradation(hops).map(|d| d.hop_index)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn make_hop(ttl: u8, sent: u32, received: u32) -> Hop {
        let mut h = Hop::new(ttl);
        h.sent = sent;
        h.received = received;
        for i in 0..received {
            h.rtt_samples.push(Duration::from_millis(10 + i as u64));
        }
        h
    }

    fn make_hop_with_rtt(ttl: u8, sent: u32, received: u32, rtt_ms: u64) -> Hop {
        let mut h = Hop::new(ttl);
        h.sent = sent;
        h.received = received;
        for _ in 0..received {
            h.rtt_samples.push(Duration::from_millis(rtt_ms));
        }
        h
    }

    #[test]
    fn test_icmp_ratelimit_detection() {
        // Hop 2 perd 50% mais hop 3 perd 0% → rate limit sur hop 2
        let mut hops = vec![
            make_hop(1, 100, 100),
            make_hop(2, 100, 50),
            make_hop(3, 100, 100),
        ];
        flag_icmp_ratelimiting(&mut hops);
        assert!(hops[1].suspected_icmp_ratelimit);
        assert!(!hops[0].suspected_icmp_ratelimit);
        assert!(!hops[2].suspected_icmp_ratelimit);
    }

    #[test]
    fn test_train_of_ratelimited_hops() {
        // Hops 2, 3, 4 perdent tous 80% mais hop 5 perd 0% → tous sont rate-limit
        let mut hops = vec![
            make_hop(1, 100, 100),
            make_hop(2, 100, 20),
            make_hop(3, 100, 20),
            make_hop(4, 100, 20),
            make_hop(5, 100, 100),
        ];
        flag_icmp_ratelimiting(&mut hops);
        assert!(hops[1].suspected_icmp_ratelimit);
        assert!(hops[2].suspected_icmp_ratelimit);
        assert!(hops[3].suspected_icmp_ratelimit);
        assert!(!hops[4].suspected_icmp_ratelimit);
    }

    #[test]
    fn test_real_loss_at_end() {
        // Hop 2 perd 10% et tous les suivants aussi → vraie perte
        let mut hops = vec![
            make_hop(1, 100, 100),
            make_hop(2, 100, 90),
            make_hop(3, 100, 90),
        ];
        flag_icmp_ratelimiting(&mut hops);
        assert!(!hops[1].suspected_icmp_ratelimit);
    }

    #[test]
    fn test_latency_jump_not_congestion() {
        // Bond transatlantique : RTT 10 → 90 ms, mais 0% de perte
        let hops = vec![
            make_hop_with_rtt(1, 100, 100, 2),
            make_hop_with_rtt(2, 100, 100, 10),
            make_hop_with_rtt(3, 100, 100, 90),
            make_hop_with_rtt(4, 100, 100, 92),
        ];
        let degradation = find_degradation(&hops).expect("should detect a degradation");
        assert_eq!(degradation.degradation_type, DegradationType::LatencyJump);
    }

    #[test]
    fn test_congestion_detected() {
        // Bond de RTT ET perte propagée → congestion
        let mut hops = vec![
            make_hop_with_rtt(1, 100, 100, 2),
            make_hop_with_rtt(2, 100, 100, 10),
            make_hop_with_rtt(3, 100, 80, 90),
            make_hop_with_rtt(4, 100, 80, 92),
        ];
        flag_icmp_ratelimiting(&mut hops);
        let degradation = find_degradation(&hops).expect("should detect");
        assert_eq!(degradation.degradation_type, DegradationType::Congestion);
    }

    #[test]
    fn test_empty_slice_no_panic() {
        let mut hops: Vec<Hop> = vec![];
        flag_icmp_ratelimiting(&mut hops); // ne doit pas paniquer
        assert!(find_degradation(&hops).is_none());
    }

    #[test]
    fn test_single_hop_no_degradation() {
        let hops = vec![make_hop_with_rtt(1, 100, 100, 10)];
        assert!(find_degradation(&hops).is_none());
    }

    #[test]
    fn test_stable_path_no_degradation() {
        let hops = vec![
            make_hop_with_rtt(1, 100, 100, 5),
            make_hop_with_rtt(2, 100, 100, 8),
            make_hop_with_rtt(3, 100, 100, 12),
            make_hop_with_rtt(4, 100, 100, 15),
        ];
        assert!(find_degradation(&hops).is_none());
    }

    #[test]
    fn test_pure_packet_loss_without_rtt_jump() {
        // Perte propagée sans bond de RTT → PacketLoss (pas Congestion)
        let mut hops = vec![
            make_hop_with_rtt(1, 100, 100, 10),
            make_hop_with_rtt(2, 100, 100, 12),
            make_hop_with_rtt(3, 100, 50, 13),  // 50% perte, RTT stable
            make_hop_with_rtt(4, 100, 50, 14),  // perte propagée
            make_hop_with_rtt(5, 100, 50, 15),  // perte propagée
        ];
        flag_icmp_ratelimiting(&mut hops);
        let d = find_degradation(&hops).expect("doit détecter une dégradation");
        assert_eq!(d.degradation_type, DegradationType::PacketLoss);
    }

    #[test]
    fn test_is_bufferbloated_true() {
        // min=20ms, max=200ms → ratio=10 > 5, jump=180ms > 80ms, min >= 10ms
        let mut h = Hop::new(1);
        h.sent = 10;
        h.received = 10;
        h.rtt_samples.push(Duration::from_millis(20));
        h.rtt_samples.push(Duration::from_millis(200));
        assert!(is_bufferbloated(&h));
    }

    #[test]
    fn test_is_bufferbloated_false_rtt_too_low() {
        // min=1ms < 10ms → false même avec grand ratio
        let mut h = Hop::new(1);
        h.sent = 10;
        h.received = 10;
        h.rtt_samples.push(Duration::from_millis(1));
        h.rtt_samples.push(Duration::from_millis(200));
        assert!(!is_bufferbloated(&h));
    }

    #[test]
    fn test_is_bufferbloated_false_small_jump() {
        // min=20ms, max=50ms → jump=30ms < 80ms → false
        let mut h = Hop::new(1);
        h.sent = 10;
        h.received = 10;
        h.rtt_samples.push(Duration::from_millis(20));
        h.rtt_samples.push(Duration::from_millis(50));
        assert!(!is_bufferbloated(&h));
    }

    #[test]
    fn test_find_degradation_skips_timeout_hops() {
        // Hop 2 est un timeout complet (aucun sample RTT) → ne doit pas bloquer la détection
        let mut hops = vec![
            make_hop_with_rtt(1, 100, 100, 10),
            make_hop(2, 100, 0),               // timeout complet, 0 RTT sample
            make_hop_with_rtt(3, 100, 100, 90), // bond depuis hop 1 (10→90ms)
            make_hop_with_rtt(4, 100, 100, 92),
        ];
        flag_icmp_ratelimiting(&mut hops);
        let d = find_degradation(&hops).expect("doit détecter un bond de latence");
        assert_eq!(d.degradation_type, DegradationType::LatencyJump);
        assert_eq!(d.hop_index, 2); // hop 2 est l'index du hop timeout (skippé) — c'est hop 3 (index 2) qui a le bond
    }
}
