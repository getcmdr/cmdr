//! The vocabulary: which fields exist, what kind of value each holds, and one
//! file's facts.

use std::borrow::Cow;

use jiff::civil::DateTime;
use serde::{Deserialize, Serialize};

/// Where a field comes from, the part of its name before the dot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MediaSource {
    /// Photos' EXIF, and the capture date of videos.
    Exif,
    /// Media tags: ID3, MP4 atoms, Vorbis comments, APE, RIFF and AIFF chunks.
    Tag,
}

/// One field a mask can name. Its mask name is [`MediaField::name`]
/// (`exif.taken`); across IPC it's the camelCase variant (`exifTaken`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum MediaField {
    /// When the photo or video was taken, local wall-clock time.
    ExifTaken,
    /// Make and model, the make said once.
    ExifCamera,
    /// The camera's maker.
    ExifMake,
    /// The camera's model.
    ExifModel,
    /// The lens.
    ExifLens,
    /// The ISO speed.
    ExifIso,
    /// The focal length, whole millimeters.
    ExifFocal,
    /// The image's width, pixels.
    ExifWidth,
    /// The image's height, pixels.
    ExifHeight,
    /// The EXIF orientation, 1–8.
    ExifOrientation,
    /// The track's title.
    TagTitle,
    /// The track's artist.
    TagArtist,
    /// The album's artist.
    TagAlbumArtist,
    /// The album.
    TagAlbum,
    /// The track number.
    TagTrack,
    /// The disc number.
    TagDisc,
    /// The release year.
    TagYear,
    /// The genre.
    TagGenre,
    /// The comment.
    TagComment,
}

/// What kind of value a field holds, which decides the formats it takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FieldKind {
    /// Takes the mask's date grammar (`:Y-M-D`).
    Date,
    /// Takes a digit count (`:2`).
    Number,
    /// Takes the range grammar (`:1-10`).
    Text,
}

/// One field's value in one file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MediaValue<'a> {
    /// Local wall-clock time.
    Date(DateTime),
    /// A whole number.
    Number(i64),
    /// Trimmed, never empty. Borrowed from the facts except for a composed
    /// value (`exif.camera`).
    Text(Cow<'a, str>),
}

/// Everything one read of a file found, every field at once. A field the file
/// doesn't have is `None`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MediaFacts {
    /// When it was taken, local wall-clock time.
    pub taken: Option<DateTime>,
    /// The camera's maker.
    pub make: Option<String>,
    /// The camera's model.
    pub model: Option<String>,
    /// The lens.
    pub lens: Option<String>,
    /// The ISO speed.
    pub iso: Option<i64>,
    /// The focal length, rounded to whole millimeters.
    pub focal_mm: Option<i64>,
    /// The width, pixels.
    pub width: Option<i64>,
    /// The height, pixels.
    pub height: Option<i64>,
    /// The EXIF orientation, 1–8.
    pub orientation: Option<i64>,
    /// The title.
    pub title: Option<String>,
    /// The artist.
    pub artist: Option<String>,
    /// The album's artist.
    pub album_artist: Option<String>,
    /// The album.
    pub album: Option<String>,
    /// The genre.
    pub genre: Option<String>,
    /// The comment.
    pub comment: Option<String>,
    /// The track number.
    pub track: Option<i64>,
    /// The disc number.
    pub disc: Option<i64>,
    /// The release year.
    pub year: Option<i64>,
}

/// Why a file's facts couldn't be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReadError {
    /// The session went away mid-read.
    Cancelled,
    /// The read ran past its deadline.
    TimedOut,
    /// Reading it would fetch more than `MAX_FETCHED_BYTES_PER_FILE`.
    TooLarge,
    /// The volume or the parser failed; the text is for the log only.
    Io(String),
    /// The volume can't do positioned reads, or the format isn't one we read.
    NotSupported,
}

impl MediaField {
    /// Every field, in the picker's order.
    pub const ALL: [Self; 19] = [
        Self::ExifTaken,
        Self::ExifCamera,
        Self::ExifMake,
        Self::ExifModel,
        Self::ExifLens,
        Self::ExifIso,
        Self::ExifFocal,
        Self::ExifWidth,
        Self::ExifHeight,
        Self::ExifOrientation,
        Self::TagTitle,
        Self::TagArtist,
        Self::TagAlbumArtist,
        Self::TagAlbum,
        Self::TagTrack,
        Self::TagDisc,
        Self::TagYear,
        Self::TagGenre,
        Self::TagComment,
    ];

    /// The name a mask writes (`exif.taken`).
    pub fn name(self) -> &'static str {
        match self {
            Self::ExifTaken => "exif.taken",
            Self::ExifCamera => "exif.camera",
            Self::ExifMake => "exif.make",
            Self::ExifModel => "exif.model",
            Self::ExifLens => "exif.lens",
            Self::ExifIso => "exif.iso",
            Self::ExifFocal => "exif.focal",
            Self::ExifWidth => "exif.width",
            Self::ExifHeight => "exif.height",
            Self::ExifOrientation => "exif.orientation",
            Self::TagTitle => "tag.title",
            Self::TagArtist => "tag.artist",
            Self::TagAlbumArtist => "tag.albumartist",
            Self::TagAlbum => "tag.album",
            Self::TagTrack => "tag.track",
            Self::TagDisc => "tag.disc",
            Self::TagYear => "tag.year",
            Self::TagGenre => "tag.genre",
            Self::TagComment => "tag.comment",
        }
    }

    /// The field a mask's name spells, exactly (case matters, as in every
    /// placeholder), or `None`.
    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|field| field.name() == name)
    }

    /// Where it comes from.
    pub fn source(self) -> MediaSource {
        match self {
            Self::ExifTaken
            | Self::ExifCamera
            | Self::ExifMake
            | Self::ExifModel
            | Self::ExifLens
            | Self::ExifIso
            | Self::ExifFocal
            | Self::ExifWidth
            | Self::ExifHeight
            | Self::ExifOrientation => MediaSource::Exif,
            Self::TagTitle
            | Self::TagArtist
            | Self::TagAlbumArtist
            | Self::TagAlbum
            | Self::TagTrack
            | Self::TagDisc
            | Self::TagYear
            | Self::TagGenre
            | Self::TagComment => MediaSource::Tag,
        }
    }

    /// What kind of value it holds.
    pub fn kind(self) -> FieldKind {
        match self {
            Self::ExifTaken => FieldKind::Date,
            Self::ExifIso
            | Self::ExifFocal
            | Self::ExifWidth
            | Self::ExifHeight
            | Self::ExifOrientation
            | Self::TagTrack
            | Self::TagDisc
            | Self::TagYear => FieldKind::Number,
            Self::ExifCamera
            | Self::ExifMake
            | Self::ExifModel
            | Self::ExifLens
            | Self::TagTitle
            | Self::TagArtist
            | Self::TagAlbumArtist
            | Self::TagAlbum
            | Self::TagGenre
            | Self::TagComment => FieldKind::Text,
        }
    }
}

impl MediaFacts {
    /// `field`'s value in this file, or `None` when it has none. Text comes
    /// trimmed (EXIF pads with spaces and NULs), and blank text is no value.
    pub fn get(&self, field: MediaField) -> Option<MediaValue<'_>> {
        let number = |value: Option<i64>| value.map(MediaValue::Number);
        match field {
            MediaField::ExifTaken => self.taken.map(MediaValue::Date),
            MediaField::ExifCamera => self.camera().map(MediaValue::Text),
            MediaField::ExifMake => text(self.make.as_deref()),
            MediaField::ExifModel => text(self.model.as_deref()),
            MediaField::ExifLens => text(self.lens.as_deref()),
            MediaField::ExifIso => number(self.iso),
            MediaField::ExifFocal => number(self.focal_mm),
            MediaField::ExifWidth => number(self.width),
            MediaField::ExifHeight => number(self.height),
            MediaField::ExifOrientation => number(self.orientation),
            MediaField::TagTitle => text(self.title.as_deref()),
            MediaField::TagArtist => text(self.artist.as_deref()),
            MediaField::TagAlbumArtist => text(self.album_artist.as_deref()),
            MediaField::TagAlbum => text(self.album.as_deref()),
            MediaField::TagTrack => number(self.track),
            MediaField::TagDisc => number(self.disc),
            MediaField::TagYear => number(self.year),
            MediaField::TagGenre => text(self.genre.as_deref()),
            MediaField::TagComment => text(self.comment.as_deref()),
        }
    }

    /// Make and model, the make said once: a model that already starts with the
    /// make's first word (`Canon` / `Canon EOS R5`, `NIKON CORPORATION` /
    /// `NIKON Z 6`) stands alone.
    fn camera(&self) -> Option<Cow<'_, str>> {
        let make = clean(self.make.as_deref());
        let model = clean(self.model.as_deref());
        match (make, model) {
            (Some(make), Some(model)) => {
                let brand = make.split_whitespace().next().unwrap_or(make);
                let model_brand = model.split_whitespace().next().unwrap_or(model);
                if model_brand.to_lowercase() == brand.to_lowercase() {
                    Some(Cow::Borrowed(model))
                } else {
                    Some(Cow::Owned(format!("{make} {model}")))
                }
            }
            (Some(one), None) | (None, Some(one)) => Some(Cow::Borrowed(one)),
            (None, None) => None,
        }
    }
}

/// `text`, cleaned, as a value.
fn text(text: Option<&str>) -> Option<MediaValue<'_>> {
    clean(text).map(|t| MediaValue::Text(Cow::Borrowed(t)))
}

/// `text` without the spaces and NULs around it, or `None` when nothing's left.
fn clean(text: Option<&str>) -> Option<&str> {
    let text = text?.trim_matches(|c: char| c.is_whitespace() || c == '\0');
    (!text.is_empty()).then_some(text)
}

/// The `io::Error` payload that carries a [`ReadError`] through `Read`.
#[derive(Debug)]
struct Carried(ReadError);

impl std::fmt::Display for Carried {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self.0)
    }
}

impl std::error::Error for Carried {}

impl ReadError {
    /// As an `io::Error`, for a [`ByteSource`](crate::ByteSource) or reader that
    /// must answer through `Read`; [`ReadError::from_io`] gets it back.
    pub fn into_io(self) -> std::io::Error {
        std::io::Error::other(Carried(self))
    }

    /// The `ReadError` inside `error` (from [`ReadError::into_io`]), or `Io` with
    /// its text for any other.
    pub fn from_io(error: std::io::Error) -> Self {
        match error.get_ref().and_then(|inner| inner.downcast_ref::<Carried>()) {
            Some(Carried(carried)) => carried.clone(),
            None => Self::Io(error.to_string()),
        }
    }

    /// The `ReadError` carried anywhere in `error`'s source chain (a parser
    /// wrapping the byte source's refusal in its own error), or `Io` with the
    /// chain's text.
    pub(crate) fn from_error(error: &(dyn std::error::Error + 'static)) -> Self {
        let mut text = Vec::new();
        let mut next = Some(error);
        while let Some(current) = next {
            let carried = current.downcast_ref::<Carried>().or_else(|| {
                current
                    .downcast_ref::<std::io::Error>()
                    .and_then(|io| io.get_ref())
                    .and_then(|inner| inner.downcast_ref::<Carried>())
            });
            if let Some(Carried(carried)) = carried {
                return carried.clone();
            }
            text.push(current.to_string());
            next = current.source();
        }
        Self::Io(text.join(": "))
    }
}

/// The extensions [`is_media_name`] takes, lowercase. CR3 and RAF are here
/// though no reader takes them yet: such a photo is honestly unreadable, never
/// a photo without a date.
const MEDIA_EXTENSIONS: &[&str] = &[
    // Photos (EXIF).
    "jpg", "jpeg", "heic", "heif", "tif", "tiff", "png", "webp", "dng", "cr2", "cr3", "nef", "arw", "orf", "rw2", "raf",
    // Audio (tags).
    "mp3", "m4a", "m4b", "flac", "ogg", "oga", "opus", "wav", "aif", "aiff", "ape", "wv", "mpc", "aac",
    // Video (capture date, and tags where the container has them).
    "mp4", "mov", "m4v", "3gp", "mkv", "webm", "mka",
];

/// Whether a file with this name is one we read, by its extension, in any case.
/// A leading dot belongs to the name (`.jpg` has no extension), as in the mask.
pub fn is_media_name(file_name: &str) -> bool {
    match file_name.rfind('.') {
        Some(dot) if dot > 0 => {
            let extension = &file_name[dot + 1..];
            MEDIA_EXTENSIONS
                .iter()
                .any(|known| known.eq_ignore_ascii_case(extension))
        }
        _ => false,
    }
}
