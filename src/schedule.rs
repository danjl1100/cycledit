use std::collections::BTreeMap;

use jiff::civil::Date;

use crate::git::FileEntry;

pub fn compute_schedule(
    mut entries: Vec<FileEntry>,
    cycle_days: i64,
    chunk_days: i64,
    today: Date,
) -> BTreeMap<Date, Vec<FileEntry>> {
    // Sort by (date ASC, blob_hash ASC) for deterministic scheduling.
    entries.sort_by(|a, b| {
        a.date
            .cmp(&b.date)
            .then_with(|| a.blob_hash.cmp(&b.blob_hash))
    });

    let max_per_chunk = ((chunk_days + cycle_days - 1) / cycle_days) as usize;
    let mut chunk_map: BTreeMap<Date, Vec<FileEntry>> = BTreeMap::new();

    for entry in entries {
        let earliest = entry
            .date
            .checked_add(jiff::Span::new().days(cycle_days))
            .unwrap();
        let mut chunk_date = earliest.max(today);
        loop {
            let count = chunk_map.get(&chunk_date).map_or(0, |v| v.len());
            if count < max_per_chunk {
                break;
            }
            chunk_date = chunk_date
                .checked_add(jiff::Span::new().days(chunk_days))
                .unwrap();
        }
        chunk_map.entry(chunk_date).or_default().push(entry);
    }

    chunk_map
}
