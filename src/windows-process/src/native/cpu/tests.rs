use super::{KNOWN_RECORD_BYTES, MAX_BYTES, MAX_RECORDS, parse};
use std::io;

fn record(id: u32, group: u16, logical: u8, efficiency: u8, extra: usize) -> Vec<u8> {
    let mut bytes = vec![0; KNOWN_RECORD_BYTES + extra];
    let size = u32::try_from(bytes.len()).unwrap_or_default();
    bytes[..4].copy_from_slice(&size.to_le_bytes());
    bytes[8..12].copy_from_slice(&id.to_le_bytes());
    bytes[12..14].copy_from_slice(&group.to_le_bytes());
    bytes[14] = logical;
    bytes[15] = logical / 2;
    bytes[16] = 4;
    bytes[17] = 2;
    bytes[18] = efficiency;
    bytes[19] = 15;
    bytes[20] = 7;
    bytes
}

#[test]
fn cpu_sets_preserve_group_relative_topology_classes_and_current_flags() -> io::Result<()> {
    let mut bytes = record(30, 1, 0, 0, 8);
    // Unknown future record types are skipped using their own declared size.
    bytes.extend([8, 0, 0, 0, 1, 0, 0, 0]);
    bytes.extend(record(20, 0, 1, 9, 0));
    bytes.extend(record(10, 0, 0, 9, 0));
    let sets = parse(&bytes)?;
    assert_eq!(sets.len(), 3);
    assert_eq!(sets[0].id, 10);
    assert_eq!(sets[1].logical_processor_index, 1);
    assert_eq!(sets[2].group, 1);
    assert_eq!(sets[2].efficiency_class, 0);
    for set in sets {
        assert_eq!(set.last_level_cache_index, 4);
        assert_eq!(set.numa_node_index, 2);
        assert_eq!(set.scheduling_class, 7);
        assert!(set.parked && set.allocated && set.allocated_to_target_process && set.real_time);
    }
    Ok(())
}

#[test]
fn cpu_sets_reject_malformed_duplicate_and_unbounded_records() {
    let valid = record(1, 0, 0, 0, 0);
    for length in 1..KNOWN_RECORD_BYTES {
        assert!(parse(&valid[..length]).is_err(), "length {length}");
    }
    for size in [0_u32, 7, 8, 31, 33, u32::MAX] {
        let mut bytes = valid.clone();
        bytes[..4].copy_from_slice(&size.to_le_bytes());
        assert!(parse(&bytes).is_err(), "size {size}");
    }
    for duplicate in [record(1, 1, 0, 0, 0), record(2, 0, 0, 0, 0)] {
        let mut bytes = valid.clone();
        bytes.extend(duplicate);
        assert!(parse(&bytes).is_err());
    }
    assert!(parse(&record(1, 0, 64, 0, 0)).is_err());
    assert!(parse(&vec![0; MAX_BYTES + 1]).is_err());
    let mut records = [8, 0, 0, 0, 1, 0, 0, 0].repeat(MAX_RECORDS);
    records.extend(valid);
    assert!(parse(&records).is_err());
    assert_eq!(
        parse(&[]).err().map(|error| error.kind()),
        Some(io::ErrorKind::Unsupported)
    );
    assert_eq!(
        parse(&[8, 0, 0, 0, 1, 0, 0, 0])
            .err()
            .map(|error| error.kind()),
        Some(io::ErrorKind::Unsupported)
    );
}

#[test]
fn cpu_sets_query_observes_current_windows_host_without_changing_policy() -> io::Result<()> {
    let sets = crate::cpu_sets()?;
    assert_ne!(sets, Vec::new());
    assert!(
        sets.windows(2)
            .all(|pair| (pair[0].group, pair[0].logical_processor_index)
                < (pair[1].group, pair[1].logical_processor_index))
    );
    Ok(())
}
