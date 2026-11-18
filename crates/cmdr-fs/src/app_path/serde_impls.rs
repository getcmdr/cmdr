//! The wire: an app path is a plain JSON string, byte-identical to the `String` it replaces,
//! and `bindings.ts` names it `string`.
//!
//! `Deserialize` goes through [`AppPathBuf::from_wire`], so an in-process format can't carry a
//! path the type would refuse. Persisted DTOs keep their `String` fields (no store migrates).

use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use super::{AppPath, AppPathBuf};

impl Serialize for AppPath {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl Serialize for AppPathBuf {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for AppPathBuf {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        AppPathBuf::from_wire(&text).map_err(D::Error::custom)
    }
}

impl specta::Type for AppPathBuf {
    fn definition(types: &mut specta::Types) -> specta::datatype::DataType {
        <String as specta::Type>::definition(types)
    }
}
