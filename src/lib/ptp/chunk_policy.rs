use std::time::Duration;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct DirectionChunkPolicy {
    pub(crate) initial_bytes: usize,
    pub(crate) effective_bytes: usize,
    pub(crate) ceiling_bytes: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ReadPromotion {
    pub(crate) old_bytes: usize,
    pub(crate) new_bytes: usize,
    pub(crate) sample_bytes: usize,
    pub(crate) sample_duration: Duration,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ChunkPolicy {
    pub(crate) read: DirectionChunkPolicy,
    pub(crate) write: DirectionChunkPolicy,
    read_max_packet_size: usize,
    read_sample_bytes: usize,
    read_sample_duration: Duration,
    read_successes: u8,
}

impl ChunkPolicy {
    pub(crate) fn for_transport(
        camera_ceiling_bytes: usize,
        speed: rusb::Speed,
        bulk_in_max_packet_size: usize,
        bulk_out_max_packet_size: usize,
    ) -> anyhow::Result<Self> {
        const FULL_SPEED_CHUNK_BYTES: usize = 256 * 1024;
        const DEFAULT_CHUNK_BYTES: usize = 1024 * 1024;
        anyhow::ensure!(
            bulk_in_max_packet_size != 0 && bulk_out_max_packet_size != 0,
            "PTP bulk endpoint packet size must be non-zero"
        );

        let conservative_bytes = match speed {
            rusb::Speed::Unknown | rusb::Speed::Low | rusb::Speed::Full => FULL_SPEED_CHUNK_BYTES,
            _ => DEFAULT_CHUNK_BYTES,
        };
        let direction = |max_packet_size| {
            let ceiling_bytes = align_down(camera_ceiling_bytes, max_packet_size);
            let effective_bytes =
                align_down(conservative_bytes.min(ceiling_bytes), max_packet_size);
            DirectionChunkPolicy {
                initial_bytes: effective_bytes,
                effective_bytes,
                ceiling_bytes,
            }
        };
        let read = direction(bulk_in_max_packet_size);
        let write = direction(bulk_out_max_packet_size);
        anyhow::ensure!(
            read.effective_bytes > super::ContainerInfo::SIZE
                && write.effective_bytes > super::ContainerInfo::SIZE,
            "PTP chunk size must exceed the PTP container header"
        );

        Ok(Self {
            read,
            write,
            read_max_packet_size: bulk_in_max_packet_size,
            read_sample_bytes: 0,
            read_sample_duration: Duration::ZERO,
            read_successes: 0,
        })
    }

    pub(crate) fn observe_read_only_success(
        &mut self,
        bytes: usize,
        duration: Duration,
    ) -> Option<ReadPromotion> {
        const PROMOTION_SUCCESSES: u8 = 3;

        if bytes < self.read.effective_bytes
            || duration.is_zero()
            || self.read.effective_bytes >= self.read.ceiling_bytes
        {
            return None;
        }

        self.read_successes = self.read_successes.saturating_add(1);
        self.read_sample_bytes = self.read_sample_bytes.saturating_add(bytes);
        self.read_sample_duration = self.read_sample_duration.saturating_add(duration);
        if self.read_successes < PROMOTION_SUCCESSES {
            return None;
        }

        let old_bytes = self.read.effective_bytes;
        let new_bytes = next_read_chunk(
            old_bytes,
            self.read.ceiling_bytes,
            self.read_max_packet_size,
        );
        let projected_fill_nanos = self
            .read_sample_duration
            .as_nanos()
            .saturating_mul(new_bytes as u128);
        let bulk_timeout_budget_nanos = super::PTP_BULK_TIMEOUT
            .as_nanos()
            .saturating_mul(self.read_sample_bytes as u128);
        if projected_fill_nanos > bulk_timeout_budget_nanos {
            self.reset_read_sample();
            return None;
        }
        let promotion = ReadPromotion {
            old_bytes,
            new_bytes,
            sample_bytes: self.read_sample_bytes,
            sample_duration: self.read_sample_duration,
        };
        self.read.effective_bytes = new_bytes;
        self.reset_read_sample();

        Some(promotion)
    }

    fn reset_read_sample(&mut self) {
        self.read_successes = 0;
        self.read_sample_bytes = 0;
        self.read_sample_duration = Duration::ZERO;
    }
}

fn next_read_chunk(current_bytes: usize, ceiling_bytes: usize, alignment: usize) -> usize {
    const MIB: usize = 1024 * 1024;
    const TIERS: [usize; 4] = [MIB, 4 * MIB, 8 * MIB, 16 * MIB];

    TIERS
        .into_iter()
        .map(|candidate| align_down(candidate, alignment))
        .find(|candidate| *candidate > current_bytes)
        .unwrap_or(ceiling_bytes)
        .min(ceiling_bytes)
}

fn align_down(bytes: usize, alignment: usize) -> usize {
    bytes - bytes % alignment
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::{ChunkPolicy, DirectionChunkPolicy, ReadPromotion};

    #[test]
    fn x_t5_starts_conservatively_on_superspeed() {
        const CONSERVATIVE_CHUNK_BYTES: usize = 1024 * 1024;
        const X_T5_CHUNK_CEILING_BYTES: usize = 16128 * 1024;

        let policy =
            ChunkPolicy::for_transport(X_T5_CHUNK_CEILING_BYTES, rusb::Speed::Super, 1024, 1024)
                .expect("valid endpoint packet sizes must produce a chunk policy");

        let expected = DirectionChunkPolicy {
            initial_bytes: CONSERVATIVE_CHUNK_BYTES,
            effective_bytes: CONSERVATIVE_CHUNK_BYTES,
            ceiling_bytes: X_T5_CHUNK_CEILING_BYTES,
        };
        assert_eq!(policy.read, expected);
        assert_eq!(policy.write, expected);
    }

    #[test]
    fn x_t5_starts_conservatively_on_full_speed() {
        const CONSERVATIVE_CHUNK_BYTES: usize = 256 * 1024;
        const X_T5_CHUNK_CEILING_BYTES: usize = 16128 * 1024;

        let policy =
            ChunkPolicy::for_transport(X_T5_CHUNK_CEILING_BYTES, rusb::Speed::Full, 64, 64)
                .expect("valid endpoint packet sizes must produce a chunk policy");

        let expected = DirectionChunkPolicy {
            initial_bytes: CONSERVATIVE_CHUNK_BYTES,
            effective_bytes: CONSERVATIVE_CHUNK_BYTES,
            ceiling_bytes: X_T5_CHUNK_CEILING_BYTES,
        };
        assert_eq!(policy.read, expected);
        assert_eq!(policy.write, expected);
    }

    #[test]
    fn unknown_speed_uses_the_most_conservative_initial_chunk() {
        const CONSERVATIVE_CHUNK_BYTES: usize = 256 * 1024;

        let policy = ChunkPolicy::for_transport(16128 * 1024, rusb::Speed::Unknown, 512, 512)
            .expect("valid endpoint packet sizes must produce a chunk policy");

        assert_eq!(policy.read.initial_bytes, CONSERVATIVE_CHUNK_BYTES);
        assert_eq!(policy.write.initial_bytes, CONSERVATIVE_CHUNK_BYTES);
    }

    #[test]
    fn aligns_each_direction_to_endpoint_packet_size() {
        let policy = ChunkPolicy::for_transport(1_000_000, rusb::Speed::Super, 512, 1024)
            .expect("valid endpoint packet sizes must produce a chunk policy");

        assert_eq!(
            policy.read,
            DirectionChunkPolicy {
                initial_bytes: 999_936,
                effective_bytes: 999_936,
                ceiling_bytes: 999_936,
            }
        );
        assert_eq!(
            policy.write,
            DirectionChunkPolicy {
                initial_bytes: 999_424,
                effective_bytes: 999_424,
                ceiling_bytes: 999_424,
            }
        );
    }

    #[test]
    fn rejects_zero_endpoint_packet_size() {
        let error = ChunkPolicy::for_transport(1024 * 1024, rusb::Speed::Super, 0, 1024)
            .expect_err("zero endpoint packet size must be rejected");

        assert!(error.to_string().contains("packet size must be non-zero"));
    }

    #[test]
    fn promotes_only_read_chunk_after_measured_large_successes() {
        const MIB: usize = 1024 * 1024;

        let mut policy = ChunkPolicy::for_transport(8 * MIB, rusb::Speed::Super, 1024, 1024)
            .expect("valid endpoint packet sizes must produce a chunk policy");

        let first = policy.observe_read_only_success(8 * MIB, Duration::from_secs(1));
        let second = policy.observe_read_only_success(8 * MIB, Duration::from_secs(1));
        let third = policy.observe_read_only_success(8 * MIB, Duration::from_secs(1));

        assert_eq!(
            (
                first,
                second,
                third,
                policy.read.effective_bytes,
                policy.write.effective_bytes,
            ),
            (
                None,
                None,
                Some(ReadPromotion {
                    old_bytes: MIB,
                    new_bytes: 4 * MIB,
                    sample_bytes: 24 * MIB,
                    sample_duration: Duration::from_secs(3),
                }),
                4 * MIB,
                MIB,
            )
        );
    }

    #[test]
    fn rejects_ceiling_that_cannot_fit_a_ptp_header() {
        let error = ChunkPolicy::for_transport(8, rusb::Speed::Super, 8, 8)
            .expect_err("chunk ceiling must fit a PTP container header");

        assert!(
            error
                .to_string()
                .contains("must exceed the PTP container header")
        );
    }

    #[test]
    fn reports_the_packet_aligned_promoted_read_chunk() {
        const MIB: usize = 1024 * 1024;
        const MAX_PACKET_SIZE: usize = 1000;

        let mut policy =
            ChunkPolicy::for_transport(8 * MIB, rusb::Speed::Super, MAX_PACKET_SIZE, 1024)
                .expect("valid endpoint packet sizes must produce a chunk policy");

        policy.observe_read_only_success(8 * MIB, Duration::from_secs(1));
        policy.observe_read_only_success(8 * MIB, Duration::from_secs(1));
        let promotion = policy
            .observe_read_only_success(8 * MIB, Duration::from_secs(1))
            .expect("the third qualifying read must promote the chunk");

        assert_eq!(promotion.new_bytes, 4_194_000);
        assert_eq!(policy.read.effective_bytes, promotion.new_bytes);
    }

    #[test]
    fn slow_reads_do_not_promote_beyond_the_bulk_timeout_budget() {
        const MIB: usize = 1024 * 1024;

        let mut policy = ChunkPolicy::for_transport(8 * MIB, rusb::Speed::Super, 1024, 1024)
            .expect("valid endpoint packet sizes must produce a chunk policy");

        policy.observe_read_only_success(8 * MIB, Duration::from_secs(40));
        policy.observe_read_only_success(8 * MIB, Duration::from_secs(40));
        let promotion = policy.observe_read_only_success(8 * MIB, Duration::from_secs(40));

        assert_eq!(promotion, None);
        assert_eq!(policy.read.effective_bytes, MIB);
    }
}
