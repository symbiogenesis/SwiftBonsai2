//! Bounded Windows CPU Set discovery; never changes process or thread policy.

use std::collections::BTreeSet;
use std::{io, ptr, slice};

use windows_sys::Win32::Foundation::ERROR_INSUFFICIENT_BUFFER;
use windows_sys::Win32::System::SystemInformation::GetSystemCpuSetInformation;
use windows_sys::Win32::System::Threading::GetCurrentProcess;

use crate::CpuSet;

const MAX_BYTES: usize = 1024 * 1024;
const MAX_RECORDS: usize = 4096;
const KNOWN_RECORD_BYTES: usize = 32;
const MAX_ATTEMPTS: usize = 3;

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

fn unsupported() -> io::Error {
    io::Error::new(io::ErrorKind::Unsupported, "no observed Windows CPU Sets")
}

pub(super) fn query() -> io::Result<Vec<CpuSet>> {
    let mut required = 0_u32;
    // SAFETY: The current-process pseudo handle requires no ownership or close,
    // is valid for this synchronous query and grants process-query access.
    let process = unsafe { GetCurrentProcess() };
    // SAFETY: A null buffer with zero length requests only the required size.
    // `required` is writable, process is live and reserved flags are zero.
    let initial =
        unsafe { GetSystemCpuSetInformation(ptr::null_mut(), 0, &raw mut required, process, 0) };
    if initial == 0 {
        let error = io::Error::last_os_error();
        if error.raw_os_error() != i32::try_from(ERROR_INSUFFICIENT_BUFFER).ok() {
            return Err(error);
        }
    }
    for _ in 0..MAX_ATTEMPTS {
        let requested = usize::try_from(required)
            .map_err(|_| invalid("Windows CPU Set buffer size exceeds platform bounds"))?;
        if requested == 0 {
            return Err(unsupported());
        }
        if requested > MAX_BYTES {
            return Err(invalid("Windows CPU Set buffer exceeds its byte limit"));
        }
        // u64 storage supplies the native structure's eight-byte alignment;
        // zero initialization also makes any padding safe to inspect as bytes.
        let mut storage = vec![0_u64; requested.div_ceil(size_of::<u64>())];
        let mut returned = 0_u32;
        // SAFETY: The allocated, aligned buffer is writable for at least
        // `required` bytes. Returned length is a separate writable value. The
        // API is synchronous, retains neither pointer and receives live process
        // and zero reserved flags. No typed records/unions are dereferenced.
        let success = unsafe {
            GetSystemCpuSetInformation(
                storage.as_mut_ptr().cast(),
                required,
                &raw mut returned,
                process,
                0,
            )
        };
        if success == 0 {
            let error = io::Error::last_os_error();
            if error.raw_os_error() == i32::try_from(ERROR_INSUFFICIENT_BUFFER).ok()
                && returned > required
            {
                required = returned;
                continue;
            }
            return Err(error);
        }
        let length = usize::try_from(returned)
            .map_err(|_| invalid("Windows CPU Set result exceeds platform bounds"))?;
        if length > requested {
            return Err(invalid("Windows CPU Set query returned an invalid length"));
        }
        // SAFETY: storage owns initialized, aligned bytes through the parse.
        // The successful query's returned length was checked within the exact
        // buffer length supplied to the API. Byte reads need no record alignment.
        let bytes = unsafe { slice::from_raw_parts(storage.as_ptr().cast(), length) };
        return parse(bytes);
    }
    Err(io::Error::new(
        io::ErrorKind::Interrupted,
        "Windows CPU Set size changed across bounded query retries",
    ))
}

fn parse(bytes: &[u8]) -> io::Result<Vec<CpuSet>> {
    if bytes.len() > MAX_BYTES {
        return Err(invalid("Windows CPU Set buffer exceeds its byte limit"));
    }
    let mut remaining = bytes;
    let mut result = Vec::new();
    let mut ids = BTreeSet::new();
    let mut processors = BTreeSet::new();
    let mut records = 0;
    while !remaining.is_empty() {
        records += 1;
        if records > MAX_RECORDS || remaining.len() < 8 {
            return Err(invalid("Windows CPU Set record count or header is invalid"));
        }
        let size = usize::try_from(u32::from_le_bytes([
            remaining[0],
            remaining[1],
            remaining[2],
            remaining[3],
        ]))
        .map_err(|_| invalid("Windows CPU Set record size exceeds platform bounds"))?;
        if size < 8 || size > remaining.len() {
            return Err(invalid("Windows CPU Set record size is invalid"));
        }
        let (record, next) = remaining.split_at(size);
        remaining = next;
        // CPU_SET_INFORMATION_TYPE::CpuSetInformation is zero. Other types
        // are deliberately skipped according to Microsoft's expansion contract.
        if record[4..8] != [0, 0, 0, 0] {
            continue;
        }
        if record.len() < KNOWN_RECORD_BYTES {
            return Err(invalid("Windows CPU Set record is truncated"));
        }
        let id = u32::from_le_bytes([record[8], record[9], record[10], record[11]]);
        let group = u16::from_le_bytes([record[12], record[13]]);
        let logical_processor_index = record[14];
        if logical_processor_index >= 64
            || !ids.insert(id)
            || !processors.insert((group, logical_processor_index))
        {
            return Err(invalid("Windows CPU Set identity is invalid or duplicated"));
        }
        let flags = record[19];
        result.push(CpuSet {
            id,
            group,
            logical_processor_index,
            core_index: record[15],
            last_level_cache_index: record[16],
            numa_node_index: record[17],
            efficiency_class: record[18],
            scheduling_class: record[20],
            parked: flags & 1 != 0,
            allocated: flags & 2 != 0,
            allocated_to_target_process: flags & 4 != 0,
            real_time: flags & 8 != 0,
        });
    }
    if result.is_empty() {
        return Err(unsupported());
    }
    result.sort_unstable_by_key(|record| (record.group, record.logical_processor_index));
    Ok(result)
}

#[cfg(test)]
mod tests;
