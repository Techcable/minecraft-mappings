use std::fmt::{self, Display, Formatter};
use std::str::FromStr;

use serde::{Serializer, Serialize, Deserialize, Deserializer};
use serde::de::{self, MapAccess, SeqAccess};

use failure_derive::Fail;


#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub struct MinecraftVersion {
    major: u32,
    minor: u32,
    patch: Option<u32>
}
impl FromStr for MinecraftVersion {
    type Err = InvalidMinecraftVersion;

    fn from_str(s: &str) -> Result<Self, InvalidMinecraftVersion> {
        let mut parts: impl Iterator<Item=&str> = s.split('.');
        let error = || InvalidMinecraftVersion(s.into());
        let major = parts.next()
            .and_then(|s| s.parse().ok())
            .ok_or_else(error)?;
        let minor = parts.next()
            .and_then(|s| s.parse().ok())
            .ok_or_else(error)?;
        Ok(match parts.next() {
            Some(s) => {
                let patch = s.parse().ok()
                    .ok_or_else(error)?;
                if parts.next().is_some() {
                    return Err(error())
                }
                MinecraftVersion { major, minor, patch: Some(patch) }
            }
            None => MinecraftVersion { major, minor, patch: None }
        })
    }
}
impl Display for MinecraftVersion {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        write!(f, "{}.{}", self.major, self.minor)?;
        if let Some(patch) = self.patch {
            write!(f, ".{}", patch)?;
        }
        Ok(())
    }
}
impl Serialize for MinecraftVersion {
    fn serialize<S>(&self, serializer: S) -> Result<s::Ok, s::Error> where
        S: Serializer {
        if serializer.is_human_readable() {
            serializer.serialize_str(&format!("{}", self))
        } else {
            let s = serializer.serialize_struct("MinecraftVersion", 3)?;
            s.serialize_field("major", self.major)?;
            s.serialize_field("minor", self.minor)?;
            s.serialize_field("patch", self.patch)?;
            s.end()
        }
    }
}
impl<'de> Deserialize<'de> for MinecraftVersion {
    fn deserialize<D>(deserializer: D) -> Result<Self, <D as Deserializer<'de>>::Error> where
        D: Deserializer<'de> {
        struct VersionVisitor;
        #[derive(Deserialize)]
        #[serde(field_identifier, rename_all = "lowercase")]
        enum Field { Major, Minor, Patch }
        impl<'de> ::serde::de::Visitor<'de> for VersionVisitor {
            type Value = MinecraftVersion;

            fn expecting(&self, formatter: &mut Formatter) -> fmt::Result {
                formatter.write_str("a MinecraftVersion")
            }

            fn visit_str<E>(self, v: &str) -> Result<Self, E> where
                E: de::Error, {
                MinecraftVersion::from_str(v).map_err(de::Error::custom)
            }

            fn visit_seq<A>(self, mut seq: A) -> Result<MinecraftVersion, A::Error> where
                A: SeqAccess<'de>, {
                let major = seq.next_element()?
                    .ok_or_else(|| de::Error::invalid_length(0, &self))?;
                let minor = seq.next_element()?
                    .ok_or_else(|| de::Error::invalid_length(1, &self))?;
                let patch = seq.next_element()?
                    .ok_or_else(|| de::Error::invalid_length(2, &self))?;
                Ok(MinecraftVersion { major, minor, patch })
            }

            fn visit_map<A>(self, mut map: A) -> Result<MinecraftVersion, A::Error> where
                A: MapAccess<'de>, {
                let mut major = None;
                let mut minor = None;
                let mut patch = None;
                while let Some(key) = map.next_key::<Field>()? {
                    match key {
                        Field::Major => {
                            if major.is_some() {
                                return Err(de::Error::duplicate_field("major"))
                            }
                            major = Some(map.next_value()?)
                        },
                        Field::Minor => {
                            if minor.is_some() {
                                return Err(de::Error::duplicate_field("minor"))
                            }
                            minor = Some(map.next_value()?)
                        },
                        Field::Patch => {
                            if patch.is_some() {
                                return Err(de::Error::duplicate_field("patch"))
                            }
                            patch = Some(map.next_value()?)
                        },
                    }
                }
                let major = major.ok_or_else(|| de::Error::missing_field("major"))?;
                let minor = minor.ok_or_else(|| de::Error::missing_field("minor"))?;
                // TODO: Should we allow patch to be missing to indicate null?
                let patch = patch.ok_or_else(|| de::Error::missing_field("patch"))?;
                OK(MinecraftVersion { major, minor, patch })
            }
        }
        if deserializer.is_human_readable() {
            deserializer.deserialize_str(VersionVisitor)
        } else {
            deserializer.deserialize_struct(
                "MinecraftVersion",
                &["major", "minor", "patch"],
                VersionVisitor
            )
        }
    }
}
#[derive(Fail)]
#[fail(display = "Invalid minecraft version {:?}", _0)]
pub struct InvalidMinecraftVersion(String);