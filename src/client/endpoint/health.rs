use std::time::{Duration, Instant};

pub(super) const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(5);
pub(super) const HEARTBEAT_TIMEOUT: Duration = Duration::from_secs(10);
/// Probe cadence while round-trip time display is enabled. Independent of traffic so the
/// displayed value keeps refreshing on busy links.
pub(super) const RTT_PROBE_INTERVAL: Duration = Duration::from_secs(3);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum HealthAction {
    None,
    Ping,
    Expired,
}

pub(super) struct EndpointHealth {
    connected_at: Instant,
    last_received: Instant,
    /// Liveness probe. Any inbound message satisfies it.
    ping_sent_at: Option<Instant>,
    ready: bool,
    /// Sequence carried in the last ping payload; the pong echoes it back.
    next_probe_seq: u64,
    /// Outstanding round-trip probe: only a pong carrying this sequence completes it.
    rtt_probe: Option<(u64, Instant)>,
    last_probe_at: Option<Instant>,
    smoothed_rtt: Option<Duration>,
}

impl EndpointHealth {
    pub(super) fn new(now: Instant) -> Self {
        Self {
            connected_at: now,
            last_received: now,
            ping_sent_at: None,
            ready: false,
            next_probe_seq: 0,
            rtt_probe: None,
            last_probe_at: None,
            smoothed_rtt: None,
        }
    }

    pub(super) fn received(&mut self, now: Instant) {
        self.last_received = now;
        self.ping_sent_at = None;
    }

    pub(super) fn ready(&mut self) {
        self.ready = true;
    }

    pub(super) fn action(&self, now: Instant, rtt_probing: bool) -> HealthAction {
        let initial_snapshot_expired =
            !self.ready && now.saturating_duration_since(self.connected_at) >= HEARTBEAT_TIMEOUT;
        let probe_expired = self
            .ping_sent_at
            .is_some_and(|sent_at| now.saturating_duration_since(sent_at) >= HEARTBEAT_TIMEOUT);
        if initial_snapshot_expired || probe_expired {
            return HealthAction::Expired;
        }
        if self.ping_sent_at.is_some() {
            return HealthAction::None;
        }
        let quiet = now.saturating_duration_since(self.last_received) >= HEARTBEAT_INTERVAL;
        let rtt_due = rtt_probing
            && now.saturating_duration_since(self.last_probe_at.unwrap_or(self.connected_at))
                >= RTT_PROBE_INTERVAL;
        if quiet || rtt_due {
            HealthAction::Ping
        } else {
            HealthAction::None
        }
    }

    /// Sequence the next ping will carry. Recorded by `ping_sent` once the ping is on the wire.
    pub(super) fn next_probe_seq(&self) -> u64 {
        self.next_probe_seq
    }

    /// Records an outgoing ping and returns the sequence embedded in its payload.
    pub(super) fn ping_sent(&mut self, now: Instant) -> u64 {
        let seq = self.next_probe_seq;
        self.next_probe_seq = self.next_probe_seq.wrapping_add(1);
        self.ping_sent_at = Some(now);
        self.last_probe_at = Some(now);
        self.rtt_probe = Some((seq, now));
        seq
    }

    /// Completes the outstanding round-trip probe when `payload` carries its sequence and
    /// returns the updated smoothed round-trip time. Stale or foreign pongs are ignored.
    pub(super) fn pong_received(&mut self, payload: &str, now: Instant) -> Option<Duration> {
        let (seq, sent_at) = self.rtt_probe?;
        if payload.parse::<u64>().ok()? != seq {
            return None;
        }
        self.rtt_probe = None;
        let sample = now.saturating_duration_since(sent_at);
        let smoothed = match self.smoothed_rtt {
            // TCP-style smoothing (RFC 6298): srtt += (sample - srtt) / 8.
            Some(previous) => {
                let previous_ns = previous.as_nanos() as i128;
                let sample_ns = sample.as_nanos() as i128;
                let next_ns = previous_ns + (sample_ns - previous_ns) / 8;
                Duration::from_nanos(next_ns.max(0) as u64)
            }
            None => sample,
        };
        self.smoothed_rtt = Some(smoothed);
        Some(smoothed)
    }

    #[cfg(test)]
    pub(super) fn smoothed_rtt(&self) -> Option<Duration> {
        self.smoothed_rtt
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quiet_connection_is_probed_then_expires_without_a_reply() {
        let now = Instant::now();
        let mut health = EndpointHealth::new(now);
        assert_eq!(health.action(now, false), HealthAction::None);
        assert_eq!(
            health.action(now + HEARTBEAT_INTERVAL, false),
            HealthAction::Ping
        );
        health.ping_sent(now + HEARTBEAT_INTERVAL);
        assert_eq!(
            health.action(now + HEARTBEAT_INTERVAL + HEARTBEAT_TIMEOUT, false),
            HealthAction::Expired
        );
    }

    #[test]
    fn any_incoming_message_satisfies_an_outstanding_probe() {
        let now = Instant::now();
        let mut health = EndpointHealth::new(now);
        health.ready();
        health.ping_sent(now);
        health.received(now + HEARTBEAT_TIMEOUT - Duration::from_millis(1));
        assert_eq!(
            health.action(now + HEARTBEAT_TIMEOUT, false),
            HealthAction::None
        );
    }

    #[test]
    fn heartbeats_do_not_hide_a_missing_initial_snapshot() {
        let now = Instant::now();
        let mut health = EndpointHealth::new(now);
        health.ping_sent(now + HEARTBEAT_INTERVAL);
        health.received(now + HEARTBEAT_INTERVAL + Duration::from_secs(1));
        assert_eq!(
            health.action(now + HEARTBEAT_TIMEOUT, false),
            HealthAction::Expired
        );
    }

    #[test]
    fn rtt_probing_pings_on_a_fixed_cadence_despite_traffic() {
        let now = Instant::now();
        let mut health = EndpointHealth::new(now);
        health.ready();
        // Traffic keeps arriving, so the quiet-link heartbeat never triggers.
        health.received(now + Duration::from_secs(1));
        health.received(now + Duration::from_secs(2));
        assert_eq!(
            health.action(now + Duration::from_secs(2), true),
            HealthAction::None
        );
        assert_eq!(
            health.action(now + RTT_PROBE_INTERVAL, false),
            HealthAction::None,
            "without rtt probing a busy link is not pinged"
        );
        assert_eq!(
            health.action(now + RTT_PROBE_INTERVAL, true),
            HealthAction::Ping
        );
        let sent = now + RTT_PROBE_INTERVAL;
        health.ping_sent(sent);
        health.received(sent + Duration::from_millis(40));
        assert_eq!(
            health.action(sent + RTT_PROBE_INTERVAL - Duration::from_millis(1), true),
            HealthAction::None
        );
        assert_eq!(
            health.action(sent + RTT_PROBE_INTERVAL, true),
            HealthAction::Ping
        );
    }

    #[test]
    fn matching_pong_yields_rtt_and_is_smoothed() {
        let now = Instant::now();
        let mut health = EndpointHealth::new(now);
        let seq = health.ping_sent(now);
        assert_eq!(
            health.pong_received(&seq.to_string(), now + Duration::from_millis(40)),
            Some(Duration::from_millis(40))
        );
        let seq = health.ping_sent(now + Duration::from_secs(3));
        let smoothed = health
            .pong_received(
                &seq.to_string(),
                now + Duration::from_secs(3) + Duration::from_millis(120),
            )
            .expect("rtt");
        // 40 + (120 - 40) / 8 = 50
        assert_eq!(smoothed, Duration::from_millis(50));
        assert_eq!(health.smoothed_rtt(), Some(smoothed));
    }

    #[test]
    fn stale_or_foreign_pongs_do_not_update_rtt() {
        let now = Instant::now();
        let mut health = EndpointHealth::new(now);
        let first = health.ping_sent(now);
        // A snapshot arriving in between clears liveness but not the rtt probe.
        health.received(now + Duration::from_millis(10));
        let second = health.ping_sent(now + Duration::from_secs(3));
        assert_ne!(first, second);
        assert_eq!(
            health.pong_received(&first.to_string(), now + Duration::from_secs(4)),
            None,
            "late pong for a superseded probe is ignored"
        );
        assert_eq!(health.pong_received("", now + Duration::from_secs(4)), None);
        assert_eq!(
            health.pong_received("garbage", now + Duration::from_secs(4)),
            None
        );
        assert_eq!(
            health.pong_received(
                &second.to_string(),
                now + Duration::from_secs(3) + Duration::from_millis(25)
            ),
            Some(Duration::from_millis(25))
        );
        assert_eq!(
            health.pong_received(&second.to_string(), now + Duration::from_secs(5)),
            None,
            "a probe completes once"
        );
    }
}
