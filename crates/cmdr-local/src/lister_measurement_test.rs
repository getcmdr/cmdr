//! TEMPORARY: prices Windows listers on the hosted runner's NTFS, printing a table the job's
//! report keeps (`store-success-output`). Goes once the numbers are in `docs/notes/`.

#![cfg(target_os = "windows")] // Win32 directory enumeration on NTFS
#![allow(clippy::print_stdout, reason = "a measurement prints its measurements")]

use std::os::windows::ffi::OsStrExt as _;
use std::path::Path;
use std::time::{Duration, Instant};

use cmdr_fs::volume::host::VolumeHost;

const FILES: usize = 20_000;
const ROUNDS: usize = 7;

fn current(dir: &Path) -> usize {
    crate::reading::list_directory_core(dir, VolumeHost::detached().listings())
        .expect("lists")
        .len()
}

fn read_dir_lstat(dir: &Path) -> usize {
    let mut count = 0;
    for entry in std::fs::read_dir(dir).expect("lists") {
        std::hint::black_box(
            std::fs::symlink_metadata(entry.expect("entry").path())
                .expect("stat")
                .len(),
        );
        count += 1;
    }
    count
}

fn read_dir_entry_metadata(dir: &Path) -> usize {
    let mut count = 0;
    for entry in std::fs::read_dir(dir).expect("lists") {
        std::hint::black_box(entry.expect("entry").metadata().expect("meta").len());
        count += 1;
    }
    count
}

type Method<'a> = (&'a str, &'a dyn Fn(&Path) -> usize);

fn find_first(dir: &Path, flags: u32) -> usize {
    use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
    use windows_sys::Win32::Storage::FileSystem::{
        FindClose, FindExInfoBasic, FindExSearchNameMatch, FindFirstFileExW, FindNextFileW, WIN32_FIND_DATAW,
    };
    let pattern: Vec<u16> = dir.join("*").as_os_str().encode_wide().chain(Some(0)).collect();
    // SAFETY: plain old data.
    let mut data: WIN32_FIND_DATAW = unsafe { std::mem::zeroed() };
    // SAFETY: NUL-terminated pattern, `data` is the struct `FindExInfoBasic` fills.
    let handle = unsafe {
        FindFirstFileExW(
            pattern.as_ptr(),
            FindExInfoBasic,
            (&raw mut data).cast(),
            FindExSearchNameMatch,
            std::ptr::null(),
            flags,
        )
    };
    assert_ne!(handle, INVALID_HANDLE_VALUE);
    let mut count = 1;
    let mut sizes = u64::from(data.nFileSizeLow);
    // SAFETY: a live search handle and the same struct.
    while unsafe { FindNextFileW(handle, &raw mut data) } != 0 {
        count += 1;
        sizes += u64::from(data.nFileSizeLow);
    }
    // SAFETY: the live handle.
    unsafe { FindClose(handle) };
    std::hint::black_box(sizes);
    count - 2 // `.` and `..`
}

fn median(mut runs: Vec<Duration>) -> Duration {
    runs.sort();
    runs[runs.len() / 2]
}

#[test]
fn zz_temporary_price_the_windows_listers() {
    let dir = cmdr_fs::testing::TestDir::new("lister_measurement");
    let started = Instant::now();
    for i in 0..FILES {
        std::fs::write(dir.join(format!("file_{i:05}.txt")), b"x").expect("write");
    }
    println!("MEASURE fixture: {FILES} files in {:?}", started.elapsed()); // allowed-pluralize-noun: a constant past one
    const LARGE_FETCH: u32 = 2;
    let methods: [Method<'_>; 5] = [
        ("current (read_dir + lstat per entry, FileEntry)", &current),
        ("read_dir + symlink_metadata per entry", &read_dir_lstat),
        ("read_dir + DirEntry::metadata (find data)", &read_dir_entry_metadata),
        ("FindFirstFileExW basic", &|d| find_first(d, 0)),
        ("FindFirstFileExW basic + LARGE_FETCH", &|d| find_first(d, LARGE_FETCH)),
    ];
    let mut times: Vec<Vec<Duration>> = vec![Vec::new(); methods.len()];
    for round in 0..=ROUNDS {
        for (i, (_, method)) in methods.iter().enumerate() {
            let at = Instant::now();
            let n = method(&dir);
            let took = at.elapsed();
            assert!(n >= FILES, "{n}");
            if round > 0 {
                times[i].push(took);
            }
        }
    }
    for ((name, _), runs) in methods.iter().zip(times) {
        println!("MEASURE {name}: median {:?} over {ROUNDS} runs", median(runs)); // allowed-pluralize-noun: a constant past one
    }
}
