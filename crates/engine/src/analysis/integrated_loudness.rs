const LUFS_OFFSET: f64 = -0.691;
const ABSOLUTE_GATE_LUFS: f64 = -70.0;
const MAX_BINS: usize = 4_096;
const MAX_ERROR_LU: f64 = 0.02;

#[derive(Clone, Copy)]
struct Bin {
    energy: f64,
    count: u64,
    min_lufs: f64,
    max_lufs: f64,
}

/// Keeps distinct block levels until the fixed bin budget is exhausted, then
/// merges the closest adjacent ranges. Energy sums are never quantized.
/// An ambiguous gate crossing returns no measurement unless the whole range
/// of possible results fits within +/- MAX_ERROR_LU of its midpoint.
pub(super) struct IntegratedLoudness {
    bins: Vec<Bin>,
    energy: f64,
    count: u64,
}

impl IntegratedLoudness {
    pub(super) fn new() -> Self {
        Self {
            bins: Vec::with_capacity(MAX_BINS),
            energy: 0.0,
            count: 0,
        }
    }

    pub(super) fn record(&mut self, momentary_lufs: f64) {
        if !momentary_lufs.is_finite() || momentary_lufs <= ABSOLUTE_GATE_LUFS {
            return;
        }

        let energy = 10.0_f64.powf((momentary_lufs - LUFS_OFFSET) / 10.0);

        if !energy.is_finite() {
            return;
        }

        let index = self.bin_position(momentary_lufs);
        let index = if self
            .bins
            .get(index)
            .is_some_and(|bin| bin.min_lufs <= momentary_lufs)
        {
            index
        } else {
            if self.bins.len() == MAX_BINS {
                self.merge_closest_bins();
            }

            self.bin_position(momentary_lufs)
        };

        if let Some(bin) = self.bins.get_mut(index)
            && bin.min_lufs <= momentary_lufs
        {
            bin.energy += energy;
            bin.count += 1;
        } else {
            self.bins.insert(
                index,
                Bin {
                    energy,
                    count: 1,
                    min_lufs: momentary_lufs,
                    max_lufs: momentary_lufs,
                },
            );
        }

        self.energy += energy;
        self.count += 1;
    }

    fn bin_position(&self, lufs: f64) -> usize {
        self.bins.partition_point(|bin| bin.max_lufs < lufs)
    }

    fn merge_closest_bins(&mut self) {
        let index = self
            .bins
            .windows(2)
            .enumerate()
            .min_by(|(_, left), (_, right)| {
                let left_width = left[1].max_lufs - left[0].min_lufs;
                let right_width = right[1].max_lufs - right[0].min_lufs;

                left_width.total_cmp(&right_width)
            })
            .map(|(index, _)| index)
            .expect("merging requires at least two occupied bins");
        let right = self.bins.remove(index + 1);
        let left = &mut self.bins[index];
        left.energy += right.energy;
        left.count += right.count;
        left.max_lufs = right.max_lufs;
    }

    pub(super) fn integrated_lufs(&self) -> Option<f64> {
        if self.count == 0 {
            return None;
        }

        let relative_threshold = self.energy / self.count as f64 / 10.0;
        let gate_lufs = lufs_from_energy(relative_threshold).max(ABSOLUTE_GATE_LUFS);
        let mut energy = 0.0;
        let mut count = 0;
        let mut boundary = None;

        for bin in &self.bins {
            if bin.max_lufs <= gate_lufs {
                continue;
            }

            if bin.min_lufs > gate_lufs {
                energy += bin.energy;
                count += bin.count;
            } else {
                // Sorted, disjoint ranges allow at most one crossing bin.
                boundary = Some(bin);
            }
        }

        let Some(boundary) = boundary else {
            return (count > 0).then(|| lufs_from_energy(energy / count as f64));
        };

        // Including the entire boundary range gives a lower bound: removing
        // its samples below the gate can only increase the gated mean. Dropping
        // the entire range gives an upper bound, since all later bins are louder.
        let lower = lufs_from_energy((energy + boundary.energy) / (count + boundary.count) as f64)
            .max(gate_lufs);
        let upper = if count > 0 {
            lufs_from_energy(energy / count as f64)
        } else {
            boundary.max_lufs
        };

        (upper - lower <= 2.0 * MAX_ERROR_LU).then_some(lower + (upper - lower) / 2.0)
    }
}

fn lufs_from_energy(energy: f64) -> f64 {
    LUFS_OFFSET + 10.0 * energy.log10()
}

#[cfg(test)]
mod tests {
    use std::mem::size_of;

    use super::*;

    fn gate_crossing_case() -> (IntegratedLoudness, f64) {
        let energy = |lufs: f64| 10.0_f64.powf((lufs - LUFS_OFFSET) / 10.0);
        let below = -50.008;
        let above = -50.002;
        let gate = -50.004;
        let loud_energy =
            201.0 * 10.0 * energy(gate) - 100.0 * energy(below) - 100.0 * energy(above);
        let mut histogram = IntegratedLoudness::new();

        for _ in 0..100 {
            histogram.record(below);
            histogram.record(above);
        }

        histogram.record(lufs_from_energy(loud_energy));
        let exact = lufs_from_energy((loud_energy + 100.0 * energy(above)) / 101.0);

        (histogram, exact)
    }

    #[test]
    fn keeps_nearby_values_on_opposite_sides_of_the_gate_separate() {
        let (histogram, exact) = gate_crossing_case();

        // The former fixed histogram dropped both quiet levels, giving
        // -17.43 LUFS instead of the exact -37.24 LUFS.
        assert!((histogram.integrated_lufs().unwrap() - exact).abs() < 1e-10);
    }

    #[test]
    fn reports_unavailable_when_merged_gate_values_cannot_be_resolved() {
        let (mut histogram, _) = gate_crossing_case();
        histogram.merge_closest_bins();

        assert_eq!(histogram.integrated_lufs(), None);
    }

    #[test]
    fn reports_a_bounded_estimate_when_crossing_range_is_narrow() {
        let energy = |lufs: f64| 10.0_f64.powf((lufs - LUFS_OFFSET) / 10.0);
        let below = energy(-50.008);
        let above = energy(-50.002);
        let loud_energy = (10_002.0 * 10.0 * energy(-50.004) - below - above) / 10_000.0;
        let mut histogram = IntegratedLoudness::new();
        histogram.record(-50.008);
        histogram.record(-50.002);

        for _ in 0..10_000 {
            histogram.record(lufs_from_energy(loud_energy));
        }

        histogram.merge_closest_bins();
        let exact = lufs_from_energy((10_000.0 * loud_energy + above) / 10_001.0);
        let actual = histogram.integrated_lufs().unwrap();

        assert!((actual - exact).abs() <= MAX_ERROR_LU);
    }

    #[test]
    fn distinct_block_levels_cannot_grow_storage_beyond_the_bin_budget() {
        let mut histogram = IntegratedLoudness::new();
        let storage = histogram.bins.as_ptr();
        let capacity = histogram.bins.capacity();

        for index in 0..MAX_BINS * 3 {
            histogram.record(-65.0 + index as f64 / 1_000.0);
        }

        assert_eq!(histogram.bins.len(), MAX_BINS);
        assert_eq!(histogram.bins.capacity(), capacity);
        assert_eq!(histogram.bins.as_ptr(), storage);
        assert_eq!(histogram.count, (MAX_BINS * 3) as u64);
        assert_eq!(
            histogram.bins.iter().map(|bin| bin.count).sum::<u64>(),
            histogram.count
        );
        assert!(
            histogram
                .bins
                .windows(2)
                .all(|bins| bins[0].max_lufs < bins[1].min_lufs)
        );
    }

    #[test]
    fn silence_and_invalid_values_do_not_contribute() {
        let mut histogram = IntegratedLoudness::new();

        for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -80.0, -70.0] {
            histogram.record(value);
        }

        assert_eq!(histogram.integrated_lufs(), None);
        histogram.record(-69.99);
        assert!((histogram.integrated_lufs().unwrap() + 69.99).abs() < 1e-10);
    }

    #[test]
    fn relative_gate_excludes_quiet_blocks_and_retains_exact_energy() {
        let mut histogram = IntegratedLoudness::new();

        for _ in 0..1_000 {
            histogram.record(-23.004);
            histogram.record(-23.006);
            histogram.record(-60.0);
        }

        let expected_energy = (10.0_f64.powf((-23.004 - LUFS_OFFSET) / 10.0)
            + 10.0_f64.powf((-23.006 - LUFS_OFFSET) / 10.0))
            / 2.0;
        let expected = LUFS_OFFSET + 10.0 * expected_energy.log10();
        assert!((histogram.integrated_lufs().unwrap() - expected).abs() < 1e-10);
    }

    #[test]
    fn moving_relative_gate_matches_exact_integration_for_varied_blocks() {
        let mut histogram = IntegratedLoudness::new();
        let mut energies = Vec::new();

        for index in 0..20_000 {
            // Sweep levels across both gates, then introduce louder material
            // so the relative gate moves through previously occupied bins.
            let lufs = if index < 10_000 {
                -75.0 + (index * 37 % 50_000) as f64 / 1_000.0
            } else {
                -45.0 + (index * 53 % 50_000) as f64 / 1_000.0
            };
            histogram.record(lufs);

            if lufs > ABSOLUTE_GATE_LUFS {
                energies.push(10.0_f64.powf((lufs - LUFS_OFFSET) / 10.0));
            }

            if index % 100 == 99 {
                if energies.is_empty() {
                    assert_eq!(histogram.integrated_lufs(), None);

                    continue;
                }

                let threshold = energies.iter().sum::<f64>() / energies.len() as f64 / 10.0;
                let mut energy = 0.0;
                let mut count = 0;

                for value in &energies {
                    if *value > threshold {
                        energy += value;
                        count += 1;
                    }
                }

                let exact = LUFS_OFFSET + 10.0 * (energy / count as f64).log10();
                assert!((histogram.integrated_lufs().unwrap() - exact).abs() < 0.02);
            }
        }
    }

    #[test]
    fn eleven_days_of_blocks_use_the_same_storage_and_keep_early_material() {
        let mut histogram = IntegratedLoudness::new();
        let storage = histogram.bins.as_ptr();
        let bytes = histogram.bins.capacity() * size_of::<Bin>();
        histogram.record(0.0);

        for _ in 0..10_000_000 {
            histogram.record(-60.0);
        }

        assert_eq!(histogram.count, 10_000_001);
        assert_eq!(histogram.bins.as_ptr(), storage);
        assert_eq!(histogram.bins.capacity() * size_of::<Bin>(), bytes);
        assert!(bytes < 200_000);
        let expected = -60.0 + 10.0 * (11_000_000.0_f64 / 10_000_001.0).log10();
        assert!((histogram.integrated_lufs().unwrap() - expected).abs() < 1e-8);
    }
}
