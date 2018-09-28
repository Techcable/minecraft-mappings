use std::cell::RefCell;

use indexmap::{IndexMap};
use failure::{Error, format_err};
use failure_derive::Fail;
use mappings::cache::MinecraftMappingsCache;
use mappings::{MinecraftVersion, McpVersion};
use srglib::prelude::*;

use super::target::{TargetMapping, TargetFilter, MappingSystem};

// These are the 'basic' mappings that we use as the basis for computing all others
const OBF2SRG: TargetMapping = TargetMapping::new(MappingSystem::Obf, MappingSystem::Srg);
const OBF2SPIGOT: TargetMapping = TargetMapping::new(MappingSystem::Obf, MappingSystem::Spigot);
const SRG2MCP: TargetMapping = TargetMapping::new(MappingSystem::Srg, MappingSystem::Mcp);
// Here are some other mapping targets, which indirectly derive from the basic mappings
const SRG2OBF: TargetMapping = OBF2SRG.reversed();
const OBF2MCP: TargetMapping = TargetMapping::new(MappingSystem::Obf, MappingSystem::Mcp);
const MCP2OBF: TargetMapping = OBF2MCP.reversed();
const SPIGOT2OBF: TargetMapping = OBF2SPIGOT.reversed();

pub struct MappingsTargetComputer<'a> {
    cache: &'a MinecraftMappingsCache,
    minecraft_version: MinecraftVersion,
    mcp_version: Option<McpVersion>,
    computed_targets: RefCell<IndexMap<TargetMapping, FrozenMappings>>,
}
impl<'a> MappingsTargetComputer<'a> {
    pub fn new(
        cache: &'a MinecraftMappingsCache,
        minecraft_version: MinecraftVersion,
        mcp_version: Option<McpVersion>
    ) -> Self {
        MappingsTargetComputer { cache, minecraft_version, mcp_version, computed_targets: Default::default() }
    }
    #[inline]
    fn mcp_version(&self) -> Result<McpVersion, Error> {
        self.mcp_version.ok_or_else(|| format_err!("Unspecified MCP version"))
    }
    pub fn compute_target(&self, target: TargetMapping) -> Result<FrozenMappings, Error> {
        {
            let computed_targets =
                self.computed_targets.borrow();
            if let Some(mappings) = computed_targets.get(&target) {
                return Ok(mappings.clone())
            }
        }
        // TODO: Protection against cycles
        let mappings = self.fallback_compute_target(target)
            .map_err(|cause| TargetComputeError { target, cause })?;
        self.computed_targets.borrow_mut().insert(target, mappings.clone());
        Ok(mappings)
    }
    fn fallback_compute_target(&self, target: TargetMapping) -> Result<FrozenMappings, Error> {
        // NOTE: These relationships are currently hardcoded
        let mut mappings = match (target.original, target.renamed) {
            (MappingSystem::Srg, MappingSystem::Mcp) => {
                let obf2srg = self.compute_target(OBF2SRG)?;
                let mcp_version = self.mcp_version()?;
                let mcp_mappings = self.cache.load_mcp_mappings(mcp_version)?;
                let mut builder = SimpleMappings::default();
                // NOTE: Serage already has the class names
                for (_, serage) in obf2srg.fields() {
                    if let Some(mcp) = mcp_mappings.fields.get(&serage.name) {
                        builder.set_field_name(serage.clone(), mcp.clone());
                    }
                }
                for (_, serage) in obf2srg.methods() {
                    if let Some(mcp) = mcp_mappings.methods.get(&serage.name) {
                        builder.set_method_name(serage.clone(), mcp.clone());
                    }
                }
                builder.frozen()
            },
            (MappingSystem::Srg, MappingSystem::Spigot) => {
                let srg2obf = self.compute_target(SRG2OBF)?;
                let obf2spigot = self.compute_target(SRG2OBF)?;
                srg2obf.chain(obf2spigot)
            },
            (MappingSystem::Srg, MappingSystem::Obf) => {
                self.compute_target(OBF2SRG)?.inverted()
            },
            (MappingSystem::Mcp, MappingSystem::Srg) => {
                self.compute_target(SRG2MCP)?.inverted()
            },
            (MappingSystem::Mcp, MappingSystem::Spigot) => {
                let mcp2obf = self.compute_target(MCP2OBF)?;
                let obf2spigot = self.compute_target(OBF2SPIGOT)?;
                mcp2obf.chain(obf2spigot)
            },
            (MappingSystem::Mcp, MappingSystem::Obf) => {
                self.compute_target(OBF2MCP)?.inverted()
            },
            (MappingSystem::Spigot, MappingSystem::Srg) => {
                let spigot2obf = self.compute_target(SPIGOT2OBF)?;
                let obf2srg = self.compute_target(OBF2SRG)?;
                spigot2obf.chain(obf2srg)
            },
            (MappingSystem::Spigot, MappingSystem::Mcp) => {
                let spigot2obf = self.compute_target(SPIGOT2OBF)?;
                let obf2mcp = self.compute_target(OBF2MCP)?;
                spigot2obf.chain(obf2mcp)
            },
            (MappingSystem::Spigot, MappingSystem::Obf) => {
                self.compute_target(OBF2SPIGOT)?.inverted()
            },
            (MappingSystem::Obf, MappingSystem::Srg) => {
                self.cache.load_srg_mappings(self.minecraft_version)?
            },
            (MappingSystem::Obf, MappingSystem::Mcp) => {
                let obf2srg = self.compute_target(OBF2SRG)?;
                let srg2mcp = self.compute_target(SRG2MCP)?;
                obf2srg.chain(srg2mcp)
            },
            (MappingSystem::Obf, MappingSystem::Spigot) => {
                self.cache.load_spigot_mappings(self.minecraft_version)?
                    .chained_mappings.clone()
            }
            (MappingSystem::Srg, MappingSystem::Srg) |
            (MappingSystem::Mcp, MappingSystem::Mcp) |
            (MappingSystem::Spigot, MappingSystem::Spigot) |
            (MappingSystem::Obf, MappingSystem::Obf) => panic!("Redundant"),
        };
        self.apply_flags(target, &mut mappings)?;
        Ok(mappings)
    }
    fn apply_flags(&self, target: TargetMapping, mappings: &mut FrozenMappings) -> Result<(), Error> {
        if target.flags.is_default() { return Ok(()) }
        if target.flags.only_obf() {
            if target.original == MappingSystem::Obf {
                /*
                 * If the original is obfuscated,
                 * the modifier is redundant and we don't have to do anything
                 */
            } else {
                let original2obf_target = target.original
                    .create_target(MappingSystem::Obf);
                let original2obf = self.compute_target(original2obf_target)?;
                let mut builder = mappings.rebuild();
                builder.retain_classes(|original, _| {
                    if let Some(obf) = original2obf.get_remapped_class(original) {
                        // We only want the new mapping if the original is still obfuscated
                        original == obf
                    } else {
                        // If there is no change from the obfuscated mapping,
                        // we still want the new mapping
                        true
                    }
                });
                builder.retain_fields(|original, _| {
                    if let Some(obf) = original2obf.get_remapped_field(original) {
                        /*
                         * We only want the new field name if the original is still obfuscated.
                         * Note that this still correctly retains deobfuscated classes,
                         * since this only handles the names (not declaring types or signatures).
                         */
                        original.name == *obf.name
                    } else {
                        true // Unchanged
                    }
                });
                builder.retain_methods(|original, _| {
                    if let Some(obf) = original2obf.get_remapped_method(original) {
                        /*
                         * We only want the new method name if the original is still obfuscated.
                         * Note that this still correctly retains deobfuscated classes,
                         * since this only handles the names (not declaring types or signatures).
                         */
                        original.name == *obf.name
                    } else {
                        true // Unchanged
                    }
                });
                *mappings = builder.frozen();
            }
        }
        match target.flags.filter() {
            None => {},
            Some(TargetFilter::Classes) => {
                let mut builder = mappings.rebuild();
                builder.clear_fields();
                builder.clear_methods();
                *mappings = builder.frozen();
            },
            Some(TargetFilter::Members) => {
                let mut builder = mappings.rebuild();
                builder.clear_classes();
                *mappings = builder.frozen();
            }
        }
        Ok(())
    }
}
#[derive(Debug, Fail)]
#[fail(display = "Unable to compute {}: {}", target, cause)]
pub struct TargetComputeError {
    target: TargetMapping,
    cause: Error
}