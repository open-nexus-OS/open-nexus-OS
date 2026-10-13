// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The firmware's device-info block (TASK-0329 P1): the quirk, enhancement and feature bits
//! of the open interface in their enum order (the index is the bit), the valued features that
//! consume one parameter each in that order, and the block's parser. What the device's
//! `Capabilities` are filled from — never from a table of ours. Re-exported by [`super::fw_image`].

use super::fw_image::FwImageError;

fn u64_at(b: &[u8], off: usize) -> Option<u64> {
    Some(u64::from_le_bytes(b.get(off..off + 8)?.try_into().ok()?))
}

/// The feature indices of the open interface, in its enum order (the index is the bit).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum Feature {
    AxiAcelite = 0,
    CdmControlStreamFormat,
    ClusterGrouping,
    CommonStoreSizeInDwords,
    Compute,
    ComputeMortonCapable,
    ComputeOverlap,
    CoreidPerOs,
    DynamicDustPower,
    EccRams,
    Fbcdc,
    FbcdcAlgorithm,
    FbcdcArchitecture,
    FbcMaxDefaultDescriptors,
    FbcMaxLargeDescriptors,
    FbCdcV4,
    GpuMulticoreSupport,
    GpuVirtualisation,
    GsRtaSupport,
    IrqPerOs,
    IspMaxTilesInFlight,
    IspSamplesPerPixel,
    IspZlsD24S8PackingOglMode,
    LayoutMars,
    MaxPartitions,
    Meta,
    MetaCorememSize,
    Mips,
    NumClusters,
    NumIspIppPipes,
    NumOsids,
    NumRasterPipes,
    Pbe2InXe,
    PbvncCoreidReg,
    Perfbus,
    PerfCounterBatch,
    PhysBusWidth,
    RiscvFwProcessor,
    Roguexe,
    S7TopInfrastructure,
    SimpleInternalParameterFormat,
    SimpleInternalParameterFormatV2,
    SimpleParameterFormatVersion,
    SlcBanks,
    SlcCacheLineSizeBits,
    SlcSizeConfigurable,
    SlcSizeInKilobytes,
    SocTimer,
    SysBusSecureReset,
    Tessellation,
    TileRegionProtection,
    TileSizeX,
    TileSizeY,
    Tla,
    TpuCemDatamasterGlobalRegisters,
    TpuDmGlobalRegisters,
    TpuFilteringModeControl,
    UscMinOutputRegistersPerPix,
    VdmDrawindirect,
    VdmObjectLevelLls,
    VirtualAddressSpaceBits,
    WatchdogTimer,
    WorkgroupProtection,
    XeArchitecture,
    XeMemoryHierarchy,
    XeTpu2,
    XpuMaxRegbanksAddrWidth,
    XpuMaxSlaves,
    XpuRegisterBroadcast,
    XtTopInfrastructure,
    ZlsSubtile,
}

/// How many feature bits the interface knows.
pub const FEATURE_COUNT: u32 = Feature::ZlsSubtile as u32 + 1;

impl Feature {
    /// Whether a present feature consumes one parameter from the block (in enum order).
    pub const fn valued(self) -> bool {
        matches!(
            self,
            Self::CdmControlStreamFormat
                | Self::CommonStoreSizeInDwords
                | Self::EccRams
                | Self::Fbcdc
                | Self::FbcdcAlgorithm
                | Self::FbcdcArchitecture
                | Self::FbcMaxDefaultDescriptors
                | Self::FbcMaxLargeDescriptors
                | Self::IspMaxTilesInFlight
                | Self::IspSamplesPerPixel
                | Self::LayoutMars
                | Self::MaxPartitions
                | Self::Meta
                | Self::MetaCorememSize
                | Self::NumClusters
                | Self::NumIspIppPipes
                | Self::NumOsids
                | Self::NumRasterPipes
                | Self::PhysBusWidth
                | Self::SimpleParameterFormatVersion
                | Self::SlcBanks
                | Self::SlcCacheLineSizeBits
                | Self::SlcSizeInKilobytes
                | Self::TileSizeX
                | Self::TileSizeY
                | Self::UscMinOutputRegistersPerPix
                | Self::VirtualAddressSpaceBits
                | Self::XeArchitecture
                | Self::XpuMaxRegbanksAddrWidth
                | Self::XpuMaxSlaves
                | Self::XpuRegisterBroadcast
        )
    }

    fn from_index(i: u32) -> Option<Self> {
        if i >= FEATURE_COUNT {
            return None;
        }
        // SAFETY-free: the enum is `repr(u32)` and dense from 0 to FEATURE_COUNT − 1; a
        // transmute would do, but a match table keeps it checkable. Dense index → variant.
        const ALL: [Feature; FEATURE_COUNT as usize] = [
            Feature::AxiAcelite,
            Feature::CdmControlStreamFormat,
            Feature::ClusterGrouping,
            Feature::CommonStoreSizeInDwords,
            Feature::Compute,
            Feature::ComputeMortonCapable,
            Feature::ComputeOverlap,
            Feature::CoreidPerOs,
            Feature::DynamicDustPower,
            Feature::EccRams,
            Feature::Fbcdc,
            Feature::FbcdcAlgorithm,
            Feature::FbcdcArchitecture,
            Feature::FbcMaxDefaultDescriptors,
            Feature::FbcMaxLargeDescriptors,
            Feature::FbCdcV4,
            Feature::GpuMulticoreSupport,
            Feature::GpuVirtualisation,
            Feature::GsRtaSupport,
            Feature::IrqPerOs,
            Feature::IspMaxTilesInFlight,
            Feature::IspSamplesPerPixel,
            Feature::IspZlsD24S8PackingOglMode,
            Feature::LayoutMars,
            Feature::MaxPartitions,
            Feature::Meta,
            Feature::MetaCorememSize,
            Feature::Mips,
            Feature::NumClusters,
            Feature::NumIspIppPipes,
            Feature::NumOsids,
            Feature::NumRasterPipes,
            Feature::Pbe2InXe,
            Feature::PbvncCoreidReg,
            Feature::Perfbus,
            Feature::PerfCounterBatch,
            Feature::PhysBusWidth,
            Feature::RiscvFwProcessor,
            Feature::Roguexe,
            Feature::S7TopInfrastructure,
            Feature::SimpleInternalParameterFormat,
            Feature::SimpleInternalParameterFormatV2,
            Feature::SimpleParameterFormatVersion,
            Feature::SlcBanks,
            Feature::SlcCacheLineSizeBits,
            Feature::SlcSizeConfigurable,
            Feature::SlcSizeInKilobytes,
            Feature::SocTimer,
            Feature::SysBusSecureReset,
            Feature::Tessellation,
            Feature::TileRegionProtection,
            Feature::TileSizeX,
            Feature::TileSizeY,
            Feature::Tla,
            Feature::TpuCemDatamasterGlobalRegisters,
            Feature::TpuDmGlobalRegisters,
            Feature::TpuFilteringModeControl,
            Feature::UscMinOutputRegistersPerPix,
            Feature::VdmDrawindirect,
            Feature::VdmObjectLevelLls,
            Feature::VirtualAddressSpaceBits,
            Feature::WatchdogTimer,
            Feature::WorkgroupProtection,
            Feature::XeArchitecture,
            Feature::XeMemoryHierarchy,
            Feature::XeTpu2,
            Feature::XpuMaxRegbanksAddrWidth,
            Feature::XpuMaxSlaves,
            Feature::XpuRegisterBroadcast,
            Feature::XtTopInfrastructure,
            Feature::ZlsSubtile,
        ];
        ALL.get(i as usize).copied()
    }
}

/// The quirk bits the interface knows, in enum order (the index is the bit).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum Quirk {
    Brn44079 = 0,
    Brn47217,
    Brn48492,
    Brn48545,
    Brn49927,
    Brn50767,
    Brn51764,
    Brn62269,
    Brn63142,
    Brn63553,
    Brn66011,
    Brn71242,
}

/// The enhancement bits the interface knows, in enum order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum Enhancement {
    Ern35421 = 0,
    Ern38020,
    Ern38748,
    Ern42064,
    Ern42290,
    Ern42606,
    Ern47025,
    Ern57596,
}

/// The device-info block: three bit masks and the valued features' parameters, borrowed.
#[derive(Clone, Copy, Debug)]
pub struct DeviceInfoBlock<'a> {
    brn: &'a [u8],
    ern: &'a [u8],
    features: &'a [u8],
    params: &'a [u8],
}

fn bit(mask: &[u8], index: u32) -> bool {
    let word = (index / 64) as usize;
    let Some(w) = u64_at(mask, word * 8) else { return false };
    (w >> (index % 64)) & 1 == 1
}

impl<'a> DeviceInfoBlock<'a> {
    pub(super) fn parse(
        bytes: &'a [u8],
        header_at: usize,
        size: u32,
    ) -> Result<Self, FwImageError> {
        let start = header_at.checked_sub(size as usize).ok_or(FwImageError::DeviceInfo)?;
        let brn_words = u64_at(bytes, start).ok_or(FwImageError::DeviceInfo)? as usize;
        let ern_words = u64_at(bytes, start + 8).ok_or(FwImageError::DeviceInfo)? as usize;
        let feat_words = u64_at(bytes, start + 16).ok_or(FwImageError::DeviceInfo)? as usize;
        let param_words = u64_at(bytes, start + 24).ok_or(FwImageError::DeviceInfo)? as usize;
        let mut at = start + 32;
        let mut take = |words: usize| -> Result<&'a [u8], FwImageError> {
            let len = words.checked_mul(8).ok_or(FwImageError::DeviceInfo)?;
            let end = at.checked_add(len).ok_or(FwImageError::DeviceInfo)?;
            if end > header_at {
                return Err(FwImageError::DeviceInfo);
            }
            let s = &bytes[at..end];
            at = end;
            Ok(s)
        };
        let brn = take(brn_words)?;
        let ern = take(ern_words)?;
        let features = take(feat_words)?;
        let params = take(param_words)?;
        Ok(Self { brn, ern, features, params })
    }

    pub fn has_quirk(&self, q: Quirk) -> bool {
        bit(self.brn, q as u32)
    }

    pub fn has_enhancement(&self, e: Enhancement) -> bool {
        bit(self.ern, e as u32)
    }

    /// Whether `f` is present.
    pub fn has(&self, f: Feature) -> bool {
        bit(self.features, f as u32)
    }

    /// A valued feature's parameter: the parameters are consumed in enum order by the
    /// present valued features. `None` when absent or not valued.
    pub fn value(&self, f: Feature) -> Option<u64> {
        if !f.valued() || !self.has(f) {
            return None;
        }
        let mut param = 0usize;
        for i in 0..(f as u32) {
            let Some(other) = Feature::from_index(i) else { break };
            if other.valued() && self.has(other) {
                param += 1;
            }
        }
        u64_at(self.params, param * 8)
    }

    /// Bits set beyond what this interface knows (a newer blob; warned about, not refused).
    pub fn unknown_feature_bits(&self) -> bool {
        (FEATURE_COUNT..(self.features.len() as u32 * 8)).any(|i| bit(self.features, i))
    }
}
