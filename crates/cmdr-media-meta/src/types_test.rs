use std::borrow::Cow;

use crate::test_fixtures::at;
use crate::types::{FieldKind, MediaFacts, MediaField, MediaSource, MediaValue, ReadError, is_media_name};

#[test]
fn every_field_round_trips_through_its_name() {
    for field in MediaField::ALL {
        assert_eq!(MediaField::parse(field.name()), Some(field), "{field:?}");
    }
}

#[test]
fn names_are_source_dot_friendly_name() {
    assert_eq!(MediaField::ExifTaken.name(), "exif.taken");
    assert_eq!(MediaField::ExifCamera.name(), "exif.camera");
    assert_eq!(MediaField::ExifFocal.name(), "exif.focal");
    assert_eq!(MediaField::TagAlbumArtist.name(), "tag.albumartist");
    assert_eq!(MediaField::TagComment.name(), "tag.comment");
    for field in MediaField::ALL {
        let prefix = match field.source() {
            MediaSource::Exif => "exif.",
            MediaSource::Tag => "tag.",
        };
        assert!(field.name().starts_with(prefix), "{field:?}");
    }
}

#[test]
fn parse_takes_only_exact_names() {
    for text in [
        "",
        "exif",
        "exif.",
        "taken",
        "EXIF.taken",
        "exif.Taken",
        "exif.taken ",
        "tag.album_artist",
    ] {
        assert_eq!(MediaField::parse(text), None, "{text:?}");
    }
}

#[test]
fn all_lists_every_field_once() {
    let mut names: Vec<&str> = MediaField::ALL.iter().map(|f| f.name()).collect();
    names.sort_unstable();
    names.dedup();
    assert_eq!(names.len(), MediaField::ALL.len());
    assert_eq!(MediaField::ALL.len(), 19);
}

#[test]
fn each_field_has_its_kind() {
    assert_eq!(MediaField::ExifTaken.kind(), FieldKind::Date);
    for field in [
        MediaField::ExifIso,
        MediaField::ExifFocal,
        MediaField::ExifWidth,
        MediaField::ExifHeight,
        MediaField::ExifOrientation,
        MediaField::TagTrack,
        MediaField::TagDisc,
        MediaField::TagYear,
    ] {
        assert_eq!(field.kind(), FieldKind::Number, "{field:?}");
    }
    for field in [
        MediaField::ExifCamera,
        MediaField::ExifMake,
        MediaField::ExifModel,
        MediaField::ExifLens,
        MediaField::TagTitle,
        MediaField::TagArtist,
        MediaField::TagAlbumArtist,
        MediaField::TagAlbum,
        MediaField::TagGenre,
        MediaField::TagComment,
    ] {
        assert_eq!(field.kind(), FieldKind::Text, "{field:?}");
    }
}

#[test]
fn a_field_crosses_ipc_as_a_camel_case_name() {
    assert_eq!(serde_json::to_string(&MediaField::ExifTaken).unwrap(), "\"exifTaken\"");
    assert_eq!(
        serde_json::to_string(&MediaField::TagAlbumArtist).unwrap(),
        "\"tagAlbumArtist\""
    );
    let back: MediaField = serde_json::from_str("\"tagTrack\"").unwrap();
    assert_eq!(back, MediaField::TagTrack);
}

fn text(facts: &MediaFacts, field: MediaField) -> Option<String> {
    match facts.get(field) {
        Some(MediaValue::Text(text)) => Some(text.into_owned()),
        None => None,
        other => panic!("{field:?} isn't text: {other:?}"),
    }
}

#[test]
fn get_answers_each_field_with_its_kind_of_value() {
    let taken = at(2026, 7, 14, 9, 5, 30);
    let facts = MediaFacts {
        taken: Some(taken),
        make: Some("Apple".into()),
        model: Some("iPhone 15 Pro".into()),
        lens: Some("iPhone 15 Pro back camera 6.86mm f/1.78".into()),
        iso: Some(64),
        focal_mm: Some(7),
        width: Some(4032),
        height: Some(3024),
        orientation: Some(6),
        title: Some("Digital Love".into()),
        artist: Some("Daft Punk".into()),
        album_artist: Some("Daft Punk".into()),
        album: Some("Discovery".into()),
        genre: Some("House".into()),
        comment: Some("great".into()),
        track: Some(3),
        disc: Some(1),
        year: Some(2001),
    };
    assert_eq!(facts.get(MediaField::ExifTaken), Some(MediaValue::Date(taken)));
    assert_eq!(facts.get(MediaField::ExifIso), Some(MediaValue::Number(64)));
    assert_eq!(facts.get(MediaField::ExifFocal), Some(MediaValue::Number(7)));
    assert_eq!(facts.get(MediaField::ExifWidth), Some(MediaValue::Number(4032)));
    assert_eq!(facts.get(MediaField::ExifHeight), Some(MediaValue::Number(3024)));
    assert_eq!(facts.get(MediaField::ExifOrientation), Some(MediaValue::Number(6)));
    assert_eq!(facts.get(MediaField::TagTrack), Some(MediaValue::Number(3)));
    assert_eq!(facts.get(MediaField::TagDisc), Some(MediaValue::Number(1)));
    assert_eq!(facts.get(MediaField::TagYear), Some(MediaValue::Number(2001)));
    assert_eq!(text(&facts, MediaField::ExifMake).as_deref(), Some("Apple"));
    assert_eq!(text(&facts, MediaField::ExifModel).as_deref(), Some("iPhone 15 Pro"));
    assert_eq!(
        text(&facts, MediaField::ExifCamera).as_deref(),
        Some("Apple iPhone 15 Pro")
    );
    assert_eq!(
        text(&facts, MediaField::ExifLens).as_deref(),
        Some("iPhone 15 Pro back camera 6.86mm f/1.78")
    );
    assert_eq!(text(&facts, MediaField::TagTitle).as_deref(), Some("Digital Love"));
    assert_eq!(text(&facts, MediaField::TagArtist).as_deref(), Some("Daft Punk"));
    assert_eq!(text(&facts, MediaField::TagAlbumArtist).as_deref(), Some("Daft Punk"));
    assert_eq!(text(&facts, MediaField::TagAlbum).as_deref(), Some("Discovery"));
    assert_eq!(text(&facts, MediaField::TagGenre).as_deref(), Some("House"));
    assert_eq!(text(&facts, MediaField::TagComment).as_deref(), Some("great"));
}

#[test]
fn an_empty_file_has_no_field() {
    let facts = MediaFacts::default();
    for field in MediaField::ALL {
        assert_eq!(facts.get(field), None, "{field:?}");
    }
}

fn camera(make: Option<&str>, model: Option<&str>) -> Option<String> {
    let facts = MediaFacts {
        make: make.map(str::to_string),
        model: model.map(str::to_string),
        ..MediaFacts::default()
    };
    text(&facts, MediaField::ExifCamera)
}

#[test]
fn camera_names_the_make_once() {
    assert_eq!(
        camera(Some("Apple"), Some("iPhone 15 Pro")).as_deref(),
        Some("Apple iPhone 15 Pro")
    );
    // The model already starts with the make.
    assert_eq!(
        camera(Some("Canon"), Some("Canon EOS R5")).as_deref(),
        Some("Canon EOS R5")
    );
    // Or with its first word, in another case.
    assert_eq!(
        camera(Some("NIKON CORPORATION"), Some("NIKON Z 6")).as_deref(),
        Some("NIKON Z 6")
    );
    assert_eq!(
        camera(Some("samsung"), Some("Samsung Galaxy S24")).as_deref(),
        Some("Samsung Galaxy S24")
    );
    // A word that only begins like the make is a different word.
    assert_eq!(camera(Some("Leica"), Some("LeicaQ")).as_deref(), Some("Leica LeicaQ"));
}

#[test]
fn camera_takes_whichever_half_there_is() {
    assert_eq!(camera(Some("Apple"), None).as_deref(), Some("Apple"));
    assert_eq!(camera(None, Some("iPhone 15 Pro")).as_deref(), Some("iPhone 15 Pro"));
    assert_eq!(camera(None, None), None);
}

#[test]
fn text_is_trimmed_and_blank_text_is_no_value() {
    let facts = MediaFacts {
        make: Some("  Apple \0".into()),
        model: Some("   ".into()),
        title: Some(String::new()),
        artist: Some(" Daft Punk ".into()),
        ..MediaFacts::default()
    };
    assert_eq!(text(&facts, MediaField::ExifMake).as_deref(), Some("Apple"));
    assert_eq!(text(&facts, MediaField::ExifModel), None);
    assert_eq!(text(&facts, MediaField::ExifCamera).as_deref(), Some("Apple"));
    assert_eq!(text(&facts, MediaField::TagTitle), None);
    assert_eq!(text(&facts, MediaField::TagArtist).as_deref(), Some("Daft Punk"));
}

#[test]
fn text_borrows_from_the_facts_when_it_can() {
    let facts = MediaFacts {
        artist: Some("Daft Punk".into()),
        ..MediaFacts::default()
    };
    assert!(matches!(
        facts.get(MediaField::TagArtist),
        Some(MediaValue::Text(Cow::Borrowed(_)))
    ));
}

#[test]
fn media_names_go_by_extension_in_any_case() {
    for name in [
        "IMG_0001.JPG",
        "a.jpeg",
        "a.heic",
        "a.HEIF",
        "a.tif",
        "a.tiff",
        "a.png",
        "a.webp",
        "a.dng",
        "a.CR2",
        "a.cr3",
        "a.nef",
        "a.arw",
        "a.orf",
        "a.rw2",
        "a.raf",
        "song.mp3",
        "a.m4a",
        "a.m4b",
        "a.flac",
        "a.ogg",
        "a.oga",
        "a.opus",
        "a.wav",
        "a.aif",
        "a.aiff",
        "a.ape",
        "a.wv",
        "a.mpc",
        "a.aac",
        "clip.MOV",
        "a.mp4",
        "a.m4v",
        "a.3gp",
        "a.mkv",
        "a.webm",
        "a.mka",
        "two.dots.mp3",
    ] {
        assert!(is_media_name(name), "{name}");
    }
}

#[test]
fn other_names_are_not_media() {
    for name in [
        "notes.txt",
        "a.pdf",
        "jpg",
        ".jpg",
        "a.",
        "README",
        "a.jpg.txt",
        "a.mp3x",
        "",
    ] {
        assert!(!is_media_name(name), "{name}");
    }
}

#[test]
fn a_read_error_survives_a_trip_through_io_error() {
    for error in [
        ReadError::Cancelled,
        ReadError::TimedOut,
        ReadError::TooLarge,
        ReadError::NotSupported,
        ReadError::Io("the share went away".into()),
    ] {
        assert_eq!(ReadError::from_io(error.clone().into_io()), error);
    }
}

#[test]
fn any_other_io_error_is_io_with_its_text() {
    let error = ReadError::from_io(std::io::Error::other("connection reset"));
    assert!(
        matches!(&error, ReadError::Io(text) if text.contains("connection reset")),
        "{error:?}"
    );
    let error = ReadError::from_io(std::io::ErrorKind::UnexpectedEof.into());
    assert!(matches!(error, ReadError::Io(_)), "{error:?}");
}
