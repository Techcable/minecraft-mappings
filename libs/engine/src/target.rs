use std::str::FromStr;
use std::fmt::{self, Display, Formatter, Write};

use mappings::MinecraftVersion;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum MappingSystem {
    Srg,
    Mcp,
    Spigot,
    Obf
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
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct TargetMapping {
    pub original: MappingSystem,
    pub renamed: MappingSystem,
    pub minecraft_version: MinecraftVersion,
    pub flags: TargetFlags
}
impl FromStr for TargetMapping {
    type Err = InvalidTarget;

    fn from_str(s: &str) -> Result<Self, InvalidTarget> {
        let invalid_target = || InvalidTarget::Target(s.into());
        let first_dash = s.find('-').ok_or_else(invalid_target)?;
        let (first, remaining) = (&s[..first_dash], &s[(first_dash + 1)..]);
        let mapping_separator = first.find('2').ok_or_else(invalid_target)?;
        let original = MappingSystem::from_id(&first[..mapping_separator])
            .ok_or_else(invalid_target)?;
        let renamed = MappingSystem::from_id(&first[(mapping_separator + 1)..])
            .ok_or_else(invalid_target)?;
        let (flags, minecraft_version) = match remaining.rfind('-') {
            Some(last_dash) => {
                let version = MinecraftVersion::from_str(&remaining[(last_dash + 1)..])?;
                let flags = TargetFlags::from_str(&remaining[..last_dash])?;
                (flags, version)
            },
            None => {
                let version = MinecraftVersion::from_str(remaining)?;
                (TargetFlags::default(), version)
            }
        };
        Ok(TargetMapping { original, renamed, minecraft_version, flags })
    }
}
impl Display for TargetMapping {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        write!(f, "{}2{}", self.original.id(), self.renamed.id())?;
        let flags = format!("{}", self.flags);
        if !flags.is_empty() {
            write!(f, "-{}", flags);
        }
        write!(f, "-{}", self.minecraft_version)?;
        Ok(())
    }
}
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub struct TargetFlags {
    filter: Option<TargetFilter>,
    only_obf: bool
}
impl TargetFlags {
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
    pub fn classes(&self) -> bool {
        self.filter == Some(TargetFilter::Classes)
    }
    #[inline]
    pub fn members(&self) -> bool {
        self.filter == Some(TargetFilter::Members)
    }
    #[inline]
    pub fn only_obf(&self) -> bool {
        self.only_obf
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
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum TargetFilter {
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
        let old_version = MinecraftVersion { major: 1, minor: 8, patch: Some(8) };
        let new_version = MinecraftVersion { major: 1, minor: 13, patch: None };
        assert_eq!(TargetMapping {
            minecraft_version: old_version,
            flags: TargetFlags::default(),
            original: MappingSystem::Srg,
            renamed: MappingSystem::Mcp,
        }, "srg2mcp-1.8.8".parse().unwrap());
        assert_eq!(TargetMapping {
            minecraft_version: old_version,
            flags: TargetFlags::default(),
            original: MappingSystem::Obf,
            renamed: MappingSystem::Mcp,
        }, "obf2mcp-1.8.8".parse().unwrap());
        assert_eq!(TargetMapping {
            minecraft_version: new_version,
            flags: TargetFlags::new(false, false, true),
            original: MappingSystem::Spigot,
            renamed: MappingSystem::Mcp,
        }, "spigot2mcp-onlyobf-1.13".parse().unwrap());
        assert_eq!(TargetMapping {
            minecraft_version: new_version,
            flags: TargetFlags::new(true, false, true),
            original: MappingSystem::Spigot,
            renamed: MappingSystem::Mcp,
        }, "spigot2mcp-classes-onlyobf-1.13".parse().unwrap());
    }
}