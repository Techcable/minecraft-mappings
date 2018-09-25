use std::str::FromStr;
use std::fmt::{self, Display, Formatter, Write};

use serde::ser::{Serialize, Serializer, SerializeStruct};
use serde::de::{self, Deserialize, Deserializer, SeqAccess, MapAccess};
use serde_derive::{Serialize, Deserialize};

#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(rename = "snake_case")]
pub enum MappingSystem {
    Srg,
    Mcp,
    Spigot,
    Obf
}
impl MappingSystem {
    #[inline]
    pub fn is_mcp(self) -> bool {
        match self {
            MappingSystem::Srg | MappingSystem::Mcp => true,
            MappingSystem::Spigot | MappingSystem::Obf => false,
        }
    }
}
impl MappingSystem {
    #[inline]
    fn id(self) -> &'static str {
        match self {
            MappingSystem::Srg => "srg",
            MappingSystem::Mcp => "mcp",
            MappingSystem::Spigot => "spigot",
            MappingSystem::Obf => "obf",
        }
    }
    fn from_id(id: &str) -> Option<MappingSystem> {
        Some(match id {
            "srg" => MappingSystem::Srg,
            "mcp" => MappingSystem::Mcp,
            "spigot" => MappingSystem::Spigot,
            "obf" => MappingSystem::Obf,
            _ => return None
        })
    }
    #[inline]
    pub(crate) fn create_target(self, renamed: MappingSystem) -> TargetMapping {
        TargetMapping { original: self, renamed, flags: Default::default() }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub struct TargetMapping {
    pub original: MappingSystem,
    pub renamed: MappingSystem,
    pub flags: TargetFlags
}
impl TargetMapping {
    #[inline]
    pub const fn new(original: MappingSystem, renamed: MappingSystem) -> TargetMapping {
        TargetMapping { original, renamed, flags: TargetFlags::default() }
    }
    #[inline]
    pub const fn reversed(self) -> TargetMapping {
        // NOTE: Swap isn't const
        TargetMapping { original: self.renamed, renamed: self.original, flags: self.flags }
    }
    #[inline]
    pub fn with_default_flags(mut self) -> TargetMapping {
        self.flags = TargetFlags::default();
        self
    }
    pub fn needs_mcp_version(&self) -> bool {
        self.original.is_mcp() || self.renamed.is_mcp()
    }
}
impl FromStr for TargetMapping {
    type Err = InvalidTarget;

    fn from_str(s: &str) -> Result<Self, InvalidTarget> {
        let invalid_target = || InvalidTarget::Target(s.into());
        let first_dash = s.find('-');
        let first = first_dash.map_or(s, |index| &s[..index]);
        let mapping_separator = first.find('2').ok_or_else(invalid_target)?;
        let original = MappingSystem::from_id(&first[..mapping_separator])
            .ok_or_else(invalid_target)?;
        let renamed = MappingSystem::from_id(&first[(mapping_separator + 1)..])
            .ok_or_else(invalid_target)?;
        let flags = match first_dash {
            Some(dash) => {
                TargetFlags::from_str(&s[(dash + 1)..])?
            },
            None => {
                TargetFlags::default()
            }
        };
        Ok(TargetMapping { original, renamed, flags })
    }
}
impl Display for TargetMapping {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        write!(f, "{}2{}", self.original.id(), self.renamed.id())?;
        let flags = format!("{}", self.flags);
        if !flags.is_empty() {
            write!(f, "-{}", flags);
        }
        Ok(())
    }
}
impl<'de> Deserialize<'de> for TargetMapping {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error> where
        D: Deserializer<'de> {
        struct TargetMappingVisitor;
        impl<'de> de::Visitor<'de> for TargetMappingVisitor {
            type Value = TargetMapping;

            fn expecting(&self, formatter: &mut Formatter) -> fmt::Result {
                formatter.write_str("a TargetMapping")
            }

            #[inline]
            fn visit_str<E>(self, s: &str) -> Result<TargetMapping, E> where
                E: de::Error, {
                TargetMapping::from_str(s).map_err(E::custom)
            }

            fn visit_seq<A>(self, mut seq: A) -> Result<TargetMapping, A::Error> where
                A: SeqAccess<'de>, {
                let original = seq.next_element()?
                    .ok_or_else(|| de::Error::invalid_length(0, &self))?;
                let renamed = seq.next_element()?
                    .ok_or_else(|| de::Error::invalid_length(1, &self))?;
                let flags = seq.next_element()?
                    .unwrap_or_else(TargetFlags::default);
                Ok(TargetMapping { original, renamed, flags })

            }

            fn visit_map<A>(self, mut map: A) -> Result<TargetMapping, A::Error> where
                A: MapAccess<'de>, {
                #[derive(Deserialize)]
                #[serde(field_identifier, rename = "snake_case")]
                enum Field {
                    Original,
                    Renamed,
                    Flags
                }
                let mut original = None;
                let mut renamed = None;
                let mut flags = None;
                if let Some(key) = map.next_key::<Field>()? {
                    match key {
                        Field::Original => {
                            if original.is_some() {
                                return Err(de::Error::duplicate_field("original"))
                            }
                            original = Some(map.next_value()?);
                        },
                        Field::Renamed => {
                            if original.is_some() {
                                return Err(de::Error::duplicate_field("renamed"))
                            }
                            renamed = Some(map.next_value()?);
                        },
                        Field::Flags => {
                            if original.is_some() {
                                return Err(de::Error::duplicate_field("flags"))
                            }
                            flags = Some(map.next_value()?);
                        },
                    }
                }
                let original = original.ok_or_else(|| de::Error::missing_field("original"))?;
                let renamed = renamed.ok_or_else(|| de::Error::missing_field("renamed"))?;
                let flags = flags.unwrap_or_else(|| TargetFlags::default());
                Ok(TargetMapping { original, renamed, flags })
            }
        }
        if deserializer.is_human_readable() {
            deserializer.deserialize_str(TargetMappingVisitor)
        } else {
            deserializer.deserialize_struct(
                "TargetMapping",
                &["original", "renamed", "flags"],
                TargetMappingVisitor
            )
        }
    }
}
impl Serialize for TargetMapping {
    fn serialize<S>(&self, serializer: S) -> Result<<S as Serializer>::Ok, <S as Serializer>::Error> where
        S: Serializer {
        if serializer.is_human_readable() {
            serializer.serialize_str(&format!("{}", self))
        } else {
            let mut s = serializer.serialize_struct("TargetMapping", 3)?;
            s.serialize_field("original", &self.original)?;
            s.serialize_field("renamed", &self.renamed)?;
            if self.flags.is_default() {
                s.skip_field("flags")?;
            } else {
                s.serialize_field("flags", &self.flags)?;
            }
            s.end()
        }
    }
}
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub struct TargetFlags {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    filter: Option<TargetFilter>,
    #[serde(default, skip_serializing_if = "::std::ops::Not::not")]
    only_obf: bool
}
impl TargetFlags {
    #[inline]
    pub const fn default() -> TargetFlags {
        TargetFlags { filter: None, only_obf: false }
    }
    #[inline]
    pub fn new(classes: bool, members: bool, only_obf: bool) -> TargetFlags {
        let filter = match (classes, members) {
            (false, false) => None,
            (false, true) => Some(TargetFilter::Members),
            (true, false) => Some(TargetFilter::Classes),
            (true, true) => panic!("Can't filter both classes and members")
        };
        TargetFlags { filter, only_obf }
    }
    #[inline]
    pub fn filter(&self) -> Option<TargetFilter> {
        self.filter
    }
    #[inline]
    pub fn only_obf(&self) -> bool {
        self.only_obf
    }
    #[inline]
    pub fn is_default(&self) -> bool {
        *self == TargetFlags::default()
    }
}
impl Default for TargetFlags {
    #[inline]
    fn default() -> Self {
        TargetFlags::default()
    }
}
impl FromStr for TargetFlags {
    type Err = InvalidTarget;

    #[inline]
    fn from_str(s: &str) -> Result<TargetFlags, InvalidTarget> {
        let mut result = TargetFlags::default();
        if s.is_empty() { return Ok(result) }
        let invalid_target = || InvalidTarget::Flags(s.into());
        for flag in s.split('-') {
            match flag {
                "classes" => {
                    if result.filter.is_some() { return Err(invalid_target()) };
                    result.filter = Some(TargetFilter::Classes);
                },
                "members" => {
                    if result.filter.is_some() { return Err(invalid_target()) };
                    result.filter = Some(TargetFilter::Members);
                },
                "onlyobf" => {
                    if result.only_obf { return Err(invalid_target()) }
                    result.only_obf = true;
                },
                _ => return Err(invalid_target())
            }
        }
        Ok(result)
    }
}
impl Display for TargetFlags {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        match self.filter {
            None => {},
            Some(TargetFilter::Classes) => f.write_str("classes")?,
            Some(TargetFilter::Members) => f.write_str("members")?,
        }
        if self.only_obf {
            if self.filter.is_some() { f.write_char('-')? };
            f.write_str("onlyobf")?;
        }
        Ok(())
    }
}
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(rename = "snake_case")]
pub enum TargetFilter {
    Classes,
    Members
}

#[derive(Debug, Fail)]
pub enum InvalidTarget {
    #[fail(display = "Invalid target {:?}", _0)]
    Target(String),
    #[fail(display = "Invalid flags {:?}", _0)]
    Flags(String),
    #[fail(display = "{}", _0)]
    MinecraftVersion(#[cause] ::mappings::version::InvalidMinecraftVersion)
}
impl From<::mappings::version::InvalidMinecraftVersion> for InvalidTarget {
    #[inline]
    fn from(e: ::mappings::version::InvalidMinecraftVersion) -> Self {
        InvalidTarget::MinecraftVersion(e)
    }
}

#[cfg(test)]
mod test {
    use super::*;
    #[test]
    fn parse_flags() {
        assert_eq!(TargetFlags::default(), "".parse().unwrap());
        assert_eq!(TargetFlags::new(false, false, true), "onlyobf".parse().unwrap());
        assert_eq!(TargetFlags::new(true, false, false), "classes".parse().unwrap());
        assert_eq!(TargetFlags::new(false, true, false), "members".parse().unwrap());
        assert_eq!(TargetFlags::new(true, false, true), "classes-onlyobf".parse().unwrap());
        assert_eq!(TargetFlags::new(false, true, true), "members-onlyobf".parse().unwrap());
        assert_eq!(TargetFlags::new(true, false, true), "onlyobf-classes".parse().unwrap());
        assert_eq!(TargetFlags::new(false, true, true), "onlyobf-members".parse().unwrap());
    }
    #[test]
    fn display_flags() {
        assert_eq!(format!("{}", TargetFlags::default()), "");
        assert_eq!(format!("{}", TargetFlags::new(false, false, true)), "onlyobf");
        assert_eq!(format!("{}", TargetFlags::new(true, false, false)), "classes");
        assert_eq!(format!("{}", TargetFlags::new(false, true, false)), "members");
        assert_eq!(format!("{}", TargetFlags::new(true, false, true)), "classes-onlyobf");
        assert_eq!(format!("{}", TargetFlags::new(false, true, true)), "members-onlyobf");
    }
    #[test]
    #[should_panic(expected = "Can't filter both classes and members")]
    #[ignore] // The panic should be supressed...
    fn conflicting_filter_flags() {
        TargetFlags::new(true, true, false);
    }

    #[test]
    fn parse_target() {
        assert_eq!(TargetMapping {
            flags: TargetFlags::default(),
            original: MappingSystem::Srg,
            renamed: MappingSystem::Mcp,
        }, "srg2mcp".parse().unwrap());
        assert_eq!(TargetMapping {
            flags: TargetFlags::default(),
            original: MappingSystem::Obf,
            renamed: MappingSystem::Mcp,
        }, "obf2mcp".parse().unwrap());
        assert_eq!(TargetMapping {
            flags: TargetFlags::new(false, false, true),
            original: MappingSystem::Spigot,
            renamed: MappingSystem::Mcp,
        }, "spigot2mcp-onlyobf".parse().unwrap());
        assert_eq!(TargetMapping {
            flags: TargetFlags::new(true, false, true),
            original: MappingSystem::Spigot,
            renamed: MappingSystem::Mcp,
        }, "spigot2mcp-classes-onlyobf".parse().unwrap());
    }
}