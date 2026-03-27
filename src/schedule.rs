//! Cycle schedule computation.

use std::{collections::BTreeMap, num::NonZeroU16};

use jiff::civil::Date;

use crate::git::FileEntry;

use eyre::{Context as _, OptionExt as _};

pub use self::params::ScheduleParams;

/// Schedules the [`FileEntry`]s for the given [`ScheduleParams`]
///
/// When `cycle_end` is `Some`, overdue items are backward-filled: oldest items are
/// assigned to the furthest available slots so that completing today's items does not
/// shift items in future slots. When `cycle_end` is `None`, the existing forward-fill
/// from today is used.
///
/// # Errors
/// Returns an error if the date arithmetic overflows
pub fn compute_schedule(
    mut entries: Vec<FileEntry>,
    params: ScheduleParams,
    today: Date,
    cycle_end: Option<Date>,
) -> eyre::Result<BTreeMap<Date, Vec<FileEntry>>> {
    let cycle_days = params.get_cycle_days();
    let chunk_days = params.get_chunk_days();
    let chunk = u32::from(chunk_days.get());

    // Sort by (date ASC, blob_hash ASC) for deterministic scheduling.
    entries.sort_by(|a, b| {
        a.get_date()
            .cmp(&b.get_date())
            .then_with(|| a.get_blob_hash().cmp(b.get_blob_hash()))
    });

    let mut chunk_map: BTreeMap<Date, Vec<FileEntry>> = BTreeMap::new();

    if let Some(cycle_end) = cycle_end {
        // Path B: backward-fill overdue items, forward-fill future items.
        let (overdue, future): (Vec<_>, Vec<_>) = entries.into_iter().partition(|e| {
            e.get_date()
                .checked_add(jiff::Span::new().days(cycle_days.get()))
                .is_ok_and(|earliest| earliest <= today)
        });

        // available_slots = ceil(max(1, cycle_end − today) / chunk_days)
        // Slots are today + k*chunk_days for k = 0 .. available_slots-1 (strictly before cycle_end).
        let days_to_end = cycle_end
            .since(today)
            .wrap_err("subtract overflow (cycle_end)")?
            .get_days()
            .max(1)
            .cast_unsigned();
        let available_slots =
            usize::try_from(days_to_end.div_ceil(chunk)).wrap_err("available_slots overflow")?;

        let overdue_count = overdue.len();
        let max_per_slot = overdue_count.div_ceil(available_slots.max(1));

        // Backward-fill: oldest items go to the furthest slots; slot 0 (today) gets the rest.
        let mut overdue_iter = overdue.into_iter();
        for k in (1..available_slots).rev() {
            let offset = u32::try_from(k)
                .wrap_err("slot index overflow")?
                .checked_mul(chunk)
                .ok_or_eyre("product overflow")?;
            let slot_date = today
                .checked_add(jiff::Span::new().days(offset))
                .wrap_err("add overflow (slot_date)")?;
            let slot_entries: Vec<_> = overdue_iter.by_ref().take(max_per_slot).collect();
            if !slot_entries.is_empty() {
                chunk_map.insert(slot_date, slot_entries);
            }
        }
        let today_entries: Vec<_> = overdue_iter.collect();
        if !today_entries.is_empty() {
            chunk_map.insert(today, today_entries);
        }

        // Forward-fill future items using their natural grid snap.
        if !future.is_empty() {
            let chunks_per_cycle = usize::from(cycle_days.div_ceil(chunk_days).get());
            let max_per_chunk = future.len().div_ceil(chunks_per_cycle);
            snap_to_grid(
                future,
                today,
                cycle_days,
                chunk,
                max_per_chunk,
                &mut chunk_map,
            )?;
        }
    } else {
        // Path A: forward-fill from today (existing behaviour).
        let chunks_per_cycle = usize::from(cycle_days.div_ceil(chunk_days).get());
        let max_per_chunk = entries.len().div_ceil(chunks_per_cycle);
        snap_to_grid(
            entries,
            today,
            cycle_days,
            chunk,
            max_per_chunk,
            &mut chunk_map,
        )?;
    }

    Ok(chunk_map)
}

/// Snaps each entry to the earliest grid point `today + k*chunk_days` that has room.
fn snap_to_grid(
    entries: Vec<FileEntry>,
    today: Date,
    cycle_days: NonZeroU16,
    chunk: u32,
    max_per_chunk: usize,
    chunk_map: &mut BTreeMap<Date, Vec<FileEntry>>,
) -> eyre::Result<()> {
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
    Ok(())
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
