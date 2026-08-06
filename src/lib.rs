//! Authority-sealed bootstrap transformation boundary.
//!
//! The engine consumes the exact reader/transaction pair produced by the Sema
//! naming authority. It revalidates that pair in Core Nomos, lowers directly to
//! Logos, and proves the resulting portable archive restores to the same typed
//! value before returning either representation.

use core_ethos::bootstrap::{BootstrapBody, EthosKind};
use core_logos::{WholeLogos, WholeLogosArchiveError};
use core_nomos::{
    BootstrapSliceOneLowering, BootstrapSliceOneLoweringError, ExternalStorageProvenance,
};
use sema_translator::bootstrap::VerifiedBootstrapAssembly;

/// Transform one complete authority-sealed bootstrap assembly.
pub trait AuthoritySealedBootstrapTransformation {
    /// Lower and archive the transaction without reconstructing an earlier
    /// Ethos generation or allocating an identity.
    fn lower_bootstrap(
        &self,
        external_storage: &[ExternalStorageProvenance],
    ) -> Result<BootstrapTransformationOutcome, BootstrapTransformationError>;
}

impl AuthoritySealedBootstrapTransformation for VerifiedBootstrapAssembly {
    fn lower_bootstrap(
        &self,
        external_storage: &[ExternalStorageProvenance],
    ) -> Result<BootstrapTransformationOutcome, BootstrapTransformationError> {
        let lowering = BootstrapSliceOneLowering::new();
        let logos = match &self.transaction().decoded().document.body {
            BootstrapBody::Sema(_) => {
                lowering.lower_sema(self.reader(), self.transaction(), external_storage)?
            }
            _ => {
                if !external_storage.is_empty() {
                    return Err(
                        BootstrapTransformationError::UnexpectedExternalStorageProvenance {
                            kind: self.transaction().decoded().document.header.kind,
                            count: external_storage.len(),
                        },
                    );
                }
                lowering.lower(self.reader(), self.transaction())?
            }
        };
        let archive = logos.to_archive_bytes()?;
        if WholeLogos::from_archive_bytes(&archive)? != logos {
            return Err(BootstrapTransformationError::ArchiveRoundTripMismatch);
        }
        Ok(BootstrapTransformationOutcome { logos, archive })
    }
}

/// Complete typed and portable result of one verified bootstrap transformation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BootstrapTransformationOutcome {
    logos: WholeLogos,
    archive: Vec<u8>,
}

// Trait exception — these accessors expose one immutable operation receipt; no
// reusable behavior or authority transition is hidden behind the inherent impl.
impl BootstrapTransformationOutcome {
    /// Typed Logos value in authority-canonical declaration order.
    pub const fn logos(&self) -> &WholeLogos {
        &self.logos
    }

    /// Validated portable archive of exactly [`Self::logos`].
    pub fn archive(&self) -> &[u8] {
        &self.archive
    }
}

/// Exact refusal from the live bootstrap transformation boundary.
#[derive(Debug, thiserror::Error)]
pub enum BootstrapTransformationError {
    /// Core Nomos rejected the sealed transaction or its supported lowering.
    #[error("bootstrap lowering failed: {0}")]
    Lowering(#[from] BootstrapSliceOneLoweringError),
    /// Whole Logos could not produce or restore its portable archive.
    #[error("bootstrap Logos archive failed: {0}")]
    Archive(#[from] WholeLogosArchiveError),
    /// Non-Sema input carried storage evidence that it cannot consume.
    #[error("{kind:?} bootstrap input carried {count} unexpected external storage records")]
    UnexpectedExternalStorageProvenance { kind: EthosKind, count: usize },
    /// A portable archive restored to a different typed value.
    #[error("bootstrap Logos archive restored to different typed content")]
    ArchiveRoundTripMismatch,
}
