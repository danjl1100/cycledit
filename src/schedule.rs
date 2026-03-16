//! Cycle schedule computation.

use std::{collections::BTreeMap, num::NonZeroU16};

use jiff::civil::Date;

use crate::git::FileEntry;

use eyre::{Context as _, OptionExt as _};

pub use self::params::ScheduleParams;

/// Schedules the [`FileEntry`]s for the given [`ScheduleParams`]
///
/// # Errors
/// Returns an error if the date arithmetic overflows
pub fn compute_schedule(
    mut entries: Vec<FileEntry>,
    params: ScheduleParams,
    today: Date,
) -> eyre::Result<BTreeMap<Date, Vec<FileEntry>>> {
    let cycle_days = params.get_cycle_days();
    let chunk_days = params.get_chunk_days();

    // Sort by (date ASC, blob_hash ASC) for deterministic scheduling.
    entries.sort_by(|a, b| {
        a.get_date()
            .cmp(&b.get_date())
            .then_with(|| a.get_blob_hash().cmp(b.get_blob_hash()))
    });

    let max_per_chunk = usize::from(chunk_days.div_ceil(cycle_days).get());
    let mut chunk_map: BTreeMap<Date, Vec<FileEntry>> = BTreeMap::new();

    for entry in entries {
        let earliest = entry
            .get_date()
            .checked_add(jiff::Span::new().days(cycle_days.get()))
            .wrap_err("add overflow (earliest)")?;
        // Snap earliest up to the nearest grid point at or after earliest.
        // Grid: today + k * chunk_days for k = 0, 1, 2, …
        // try_from fails (→ 0) for overdue files where days_ahead would be negative.
        let days_ahead = u32::try_from(
            earliest
                .since(today)
                .wrap_err("subtract overflow")?
                .get_days(),
        )
        .unwrap_or(0);
        let chunk = u32::from(chunk_days.get());

        let mut k = days_ahead.div_ceil(chunk);
        let mut chunk_date;
        loop {
            let offset = k.checked_mul(chunk).ok_or_eyre("product overflow")?;
            chunk_date = today
                .checked_add(jiff::Span::new().days(offset))
                .wrap_err("add overflow (chunk_date)")?;

            let count = chunk_map.get(&chunk_date).map_or(0, Vec::len);
            if count < max_per_chunk {
                break;
            }

            k = k.checked_add(1).ok_or_eyre("add overflow (counter)")?;
        }
        chunk_map.entry(chunk_date).or_default().push(entry);
    }

    Ok(chunk_map)
}

mod params {
    //! Invariants:
    //! - `chunk_days` must be less than or equal to `cycle_days`

    use super::ChunkExceedsCycleError;
    use std::num::NonZeroU16;

    /// Duration parameters for [`crate::schedule::compute_schedule`]
    #[derive(Clone, Copy, Debug)]
    pub struct ScheduleParams {
        cycle_days: NonZeroU16,
        chunk_days: NonZeroU16,
    }
    impl ScheduleParams {
        /// Returns the number of days in the total cycle
        #[must_use]
        pub fn get_cycle_days(self) -> NonZeroU16 {
            self.cycle_days
        }
        /// Returns the number of days in each chunk (within the cycle)
        #[must_use]
        pub fn get_chunk_days(self) -> NonZeroU16 {
            self.chunk_days
        }
    }
    impl super::ScheduleParamsBuilder<NonZeroU16> {
        /// Sets the repeated chunk length in days
        ///
        /// # Errors
        ///
        /// Returns an error if the `chunk_days` is greater than `cycle_days`
        pub fn chunk_days(
            self,
            chunk_days: NonZeroU16,
        ) -> Result<ScheduleParams, ChunkExceedsCycleError> {
            let Self { cycle_days } = self;
            if chunk_days > cycle_days {
                Err(ChunkExceedsCycleError {
                    cycle_days,
                    chunk_days,
                })
            } else {
                Ok(ScheduleParams {
                    cycle_days,
                    chunk_days,
                })
            }
        }
    }
}

impl ScheduleParams {
    /// Returns a builder to construct a valid [`ScheduleParams`]
    #[must_use]
    pub fn builder() -> ScheduleParamsBuilder {
        ScheduleParamsBuilder { cycle_days: () }
    }
}
/// Builder for [`ScheduleParams`]
pub struct ScheduleParamsBuilder<T = ()> {
    cycle_days: T,
}
impl ScheduleParamsBuilder<()> {
    /// Sets the total cycle length in days
    #[must_use]
    pub fn cycle_days(self, cycle_days: NonZeroU16) -> ScheduleParamsBuilder<NonZeroU16> {
        let Self { cycle_days: () } = self;
        ScheduleParamsBuilder { cycle_days }
    }
}

/// Error for the `chunk_days` when creating [`ScheduleParams`]
#[derive(Debug)]
pub struct ChunkExceedsCycleError {
    cycle_days: NonZeroU16,
    chunk_days: NonZeroU16,
}
impl std::error::Error for ChunkExceedsCycleError {}
impl std::fmt::Display for ChunkExceedsCycleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let Self {
            cycle_days,
            chunk_days,
        } = self;
        write!(
            f,
            "chunk days ({chunk_days}) exceeds cycle days ({cycle_days})"
        )
    }
}
