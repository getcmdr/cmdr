//! The one decision, free of any platform call: may the page under this fault become zeros?

/// What the platform's memory map says about the region covering a fault address.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::mapped_file_faults) struct Region {
    /// First byte of the region.
    pub(in crate::mapped_file_faults) start: u64,
    /// One past its last byte.
    pub(in crate::mapped_file_faults) end: u64,
    /// A file backs it, rather than anonymous memory.
    pub(in crate::mapped_file_faults) file_backed: bool,
    pub(in crate::mapped_file_faults) writable: bool,
    pub(in crate::mapped_file_faults) executable: bool,
}

/// The page to map zeros over, or `None` when the fault isn't ours to recover.
///
/// `region` is what the map answered for `address`, which on macOS can be the NEXT region when
/// nothing covers `address`, so containment is checked here rather than trusted. `in_image` says
/// a loaded image (or an image table that couldn't be kept) covers `address`.
pub(in crate::mapped_file_faults) fn page_to_recover(
    address: u64,
    page_size: u64,
    region: Option<Region>,
    in_image: bool,
) -> Option<u64> {
    let region = region?;
    let covered = region.start <= address && address < region.end;
    let recoverable = covered && region.file_backed && !region.writable && !region.executable && !in_image;
    (recoverable && page_size.is_power_of_two()).then(|| address & !(page_size - 1))
}
