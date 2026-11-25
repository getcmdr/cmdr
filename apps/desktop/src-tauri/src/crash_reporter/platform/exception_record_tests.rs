//! The Windows native-crash record, on every host: the bytes the crashing process writes, read back.

use super::exception_record::*;

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().collect()
}

fn access_violation() -> Header {
    Header {
        kind: Kind::Exception,
        code: 0xC000_0005,
        address: 0x7ff6_1000_1a2b,
        param_count: 2,
        params: [0, 0x10, 0],
        image_base: 0x7ff6_1000_0000,
        uptime_ms: 12_345,
    }
}

/// A record shaped like the crash path's: the faulting frame in our image, then one in `ntdll.dll`
/// and one no image holds.
fn sample_record() -> Vec<u8> {
    let mut bytes = Vec::new();
    write_header(&mut bytes, &access_violation(), "0.60.0");
    write_module(
        &mut bytes,
        0x7ff6_1000_0000,
        &wide(r"\Device\HarddiskVolume3\Users\test_user_1\AppData\Local\Cmdr\cmdr.exe"),
    );
    write_frame(&mut bytes, 0x7ff6_1000_1a2b, 0x7ff6_1000_0000);
    write_module(
        &mut bytes,
        0x7ffa_2000_0000,
        &wide(r"\Device\HarddiskVolume3\Windows\System32\ntdll.dll"),
    );
    write_frame(&mut bytes, 0x7ffa_2000_5000, 0x7ffa_2000_0000);
    write_frame(&mut bytes, 0x1234, 0);
    bytes
}

#[test]
fn a_record_reads_back_what_the_crash_path_wrote() {
    let record = parse(&sample_record()).expect("a well-formed record parses");
    assert_eq!(record.header, access_violation());
    assert_eq!(record.app_version, "0.60.0");
    assert_eq!(record.frames.len(), 3);
    assert_eq!(record.signal(), "EXCEPTION_ACCESS_VIOLATION");
    assert_eq!(
        record.exception_line(),
        "EXCEPTION_ACCESS_VIOLATION (0xC0000005): reading 0x0000000000000010, at cmdr.exe+0x1a2b"
    );
    assert_eq!(
        record.module_frames(),
        ["cmdr.exe+0x1a2b", "ntdll.dll+0x5000", "0x0000000000001234"]
    );
    assert_eq!(record.raw_frames()[0], "0x00007ff610001a2b");
}

#[test]
fn a_module_name_never_reaches_the_disk_as_a_path() {
    let bytes = sample_record();
    let as_text = String::from_utf16_lossy(
        &bytes
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .collect::<Vec<_>>(),
    );
    let odd_text = String::from_utf16_lossy(
        &bytes[1..]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .collect::<Vec<_>>(),
    );
    for text in [as_text, odd_text] {
        assert!(!text.contains("test_user_1"), "the user's name stays off the disk");
        assert!(!text.contains("Device"), "so does the rest of the path");
    }
    let record = parse(&bytes).expect("parses");
    let names: Vec<&str> = record.modules.iter().map(|(_, name)| name.as_str()).collect();
    assert_eq!(names, ["cmdr.exe", "ntdll.dll"]);
}

#[test]
fn a_record_cut_short_mid_crash_keeps_every_whole_record_before_the_cut() {
    let bytes = sample_record();
    // Cut inside the last frame record.
    let record = parse(&bytes[..bytes.len() - 5]).expect("the header is whole");
    assert_eq!(record.frames.len(), 2);
    assert_eq!(record.module_frames(), ["cmdr.exe+0x1a2b", "ntdll.dll+0x5000"]);
}

#[test]
fn what_isnt_a_record_parses_to_none() {
    assert!(parse(&[]).is_none(), "the empty file every session leaves");
    assert!(parse(&sample_record()[..50]).is_none(), "cut inside the header");
    let mut other_magic = sample_record();
    other_magic[0..4].copy_from_slice(b"CMCR");
    assert!(parse(&other_magic).is_none(), "the Unix signal handler's format");
    let mut other_version = sample_record();
    other_version[4] = 9;
    assert!(parse(&other_version).is_none());
}

#[test]
fn an_abort_names_itself_and_claims_no_address() {
    let mut bytes = Vec::new();
    let header = Header {
        kind: Kind::Abort,
        code: 0,
        address: 0,
        param_count: 0,
        params: [0; 3],
        image_base: 0,
        uptime_ms: 0,
    };
    write_header(&mut bytes, &header, "0.60.0");
    let record = parse(&bytes).expect("parses");
    assert_eq!(record.signal(), "SIGABRT");
    assert_eq!(record.exception_line(), "abort() called");
    assert!(record.module_frames().is_empty());
}

#[test]
fn an_exception_without_a_name_reads_as_its_code() {
    let mut bytes = Vec::new();
    let header = Header {
        code: 0xE000_0001,
        param_count: 0,
        ..access_violation()
    };
    write_header(&mut bytes, &header, "0.60.0");
    let record = parse(&bytes).expect("parses");
    assert_eq!(record.signal(), "EXCEPTION 0xE0000001");
    assert_eq!(
        record.exception_line(),
        "EXCEPTION 0xE0000001 (0xE0000001), at 0x00007ff610001a2b"
    );
}

#[test]
fn an_in_page_error_carries_the_status_of_the_read_that_failed() {
    let mut bytes = Vec::new();
    let header = Header {
        code: 0xC000_0006,
        param_count: 3,
        params: [0, 0x2000_0000, 0xC000_000E],
        ..access_violation()
    };
    write_header(&mut bytes, &header, "0.60.0");
    let record = parse(&bytes).expect("parses");
    assert_eq!(
        record.exception_line(),
        "EXCEPTION_IN_PAGE_ERROR (0xC0000006): reading 0x0000000020000000 (status 0xC000000E), at 0x00007ff610001a2b"
    );
}

#[test]
fn a_long_app_version_and_module_name_are_capped_not_overflowed() {
    let mut bytes = Vec::new();
    write_header(&mut bytes, &access_violation(), &"9".repeat(40));
    write_module(&mut bytes, 0x1000, &wide(&"a".repeat(200)));
    let record = parse(&bytes).expect("parses");
    assert_eq!(record.app_version.len(), APP_VERSION_FIELD_LEN);
    assert_eq!(record.modules[0].1.len(), MODULE_NAME_MAX);
}

#[test]
fn a_frame_renders_relative_to_its_module_or_bare() {
    assert_eq!(render_frame(Some("cmdr.exe"), 0x1_0010, 0x1_0000), "cmdr.exe+0x10");
    assert_eq!(render_frame(Some(""), 0x1_0010, 0x1_0000), "unknown-module+0x10");
    assert_eq!(render_frame(None, 0x1_0010, 0x1_0000), "0x0000000000010010");
    assert_eq!(
        render_frame(Some("cmdr.exe"), 0x10, 0x1_0000),
        "0x0000000000000010",
        "below the base"
    );
}

/// The next-launch half: a record on disk becomes the report the dialog offers, in the shape every
/// other crash report has, with nothing path-shaped left in it.
#[test]
fn the_next_launch_turns_a_record_into_an_offerable_report() {
    let dir = tempfile::tempdir().expect("tempdir");
    let raw_path = dir.path().join(crate::crash_reporter::RAW_CRASH_FILE_NAME);
    let json_path = dir.path().join(crate::crash_reporter::CRASH_FILE_NAME);
    std::fs::write(&raw_path, sample_record()).expect("write the record");

    crate::crash_reporter::next_launch::convert_raw_exception_crash(&json_path, &raw_path);

    let report = crate::crash_reporter::read_crash_report(&json_path).expect("a report was written");
    assert_eq!(report.signal.as_deref(), Some("EXCEPTION_ACCESS_VIOLATION"));
    assert_eq!(report.app_fate, crate::crash_reporter::AppFate::Ended);
    assert_eq!(report.app_version, "0.60.0");
    assert_eq!(report.image_base.as_deref(), Some("0x7ff610000000"));
    assert!((report.uptime_secs - 12.345).abs() < 1e-9);
    assert_eq!(
        report.os_exception.as_deref(),
        Some("EXCEPTION_ACCESS_VIOLATION (0xC0000005): reading 0x0000000000000010, at cmdr.exe+0x1a2b")
    );
    // Through `prepare_for_delivery`'s redaction unchanged: a module name and an offset aren't
    // path-shaped.
    assert_eq!(
        report.os_frames,
        ["cmdr.exe+0x1a2b", "ntdll.dll+0x5000", "0x0000000000001234"]
    );
    assert_eq!(report.backtrace_frames.len(), 3);
    assert!(report.short_id.as_deref().is_some_and(|id| id.starts_with("CRASH-")));
    assert!(report.diag_id.starts_with("diag_"));
    assert!(report.panic_message.is_none(), "a native crash carries no message");

    let json = std::fs::read_to_string(&json_path).expect("read the report");
    assert!(!json.contains("test_user_1"));
    assert!(!json.contains(&dir.path().display().to_string()));
}

#[test]
fn an_empty_record_file_makes_no_report() {
    let dir = tempfile::tempdir().expect("tempdir");
    let raw_path = dir.path().join(crate::crash_reporter::RAW_CRASH_FILE_NAME);
    let json_path = dir.path().join(crate::crash_reporter::CRASH_FILE_NAME);
    std::fs::write(&raw_path, b"").expect("write");

    crate::crash_reporter::next_launch::convert_raw_exception_crash(&json_path, &raw_path);

    assert!(
        !json_path.exists(),
        "the file every clean session leaves offers nothing"
    );
}

/// The panic hook's half: a shipped Windows binary names none of our frames, so its report carries
/// the module-relative walk instead, which that release's PDB resolves.
#[test]
fn a_panic_backtrace_that_cant_name_our_code_gives_way_to_the_module_walk() {
    let walk = || Some(vec!["cmdr.exe+0x1a2b".to_string(), "ntdll.dll+0x5000".to_string()]);
    let unnamed = vec!["<unknown>".to_string(), "BaseThreadInitThunk".to_string()];
    assert_eq!(
        crate::crash_reporter::prefer_named_frames(unnamed.clone(), walk),
        ["cmdr.exe+0x1a2b", "ntdll.dll+0x5000"]
    );

    let named = vec!["cmdr_lib::listing::sort".to_string(), "<unknown>".to_string()];
    assert_eq!(
        crate::crash_reporter::prefer_named_frames(named.clone(), walk),
        named,
        "a build with symbols keeps std's"
    );
    assert_eq!(
        crate::crash_reporter::prefer_named_frames(unnamed.clone(), || None),
        unnamed,
        "Unix offers no walk"
    );
    assert_eq!(
        crate::crash_reporter::prefer_named_frames(unnamed.clone(), || Some(Vec::new())),
        unnamed
    );
}
