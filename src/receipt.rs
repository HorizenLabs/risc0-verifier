// Copyright Copyright 2024, Horizen Labs, Inc.
// Copyright Copyright 2024 RISC Zero, Inc.
//
// SPDX-License-Identifier: Apache-2.0
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.
//

use alloc::vec::Vec;
use composite::CompositeReceipt;
use risc0_zkp::{core::digest::Digest, verify::VerificationError};
use serde::{Deserialize, Serialize};

use crate::{
    circuit::CircuitCoreDef,
    receipt_claim::{MaybePruned, ReceiptClaim, Unknown},
    sha::{Digestible, Sha256},
    VerifierContext,
};
use succinct::SuccinctReceipt;

pub mod composite;
pub mod succinct;

pub mod merkle;
/// Maximum segment size, as a power of two (po2) that the default verifier parameters will accept.
///
/// A default of 21 was selected to reach a target of 97 bits of security under our analysis. Using
/// a po2 higher than 21 shows a degradation of 1 bit of security per po2, to 94 bits at po2 24.
pub const DEFAULT_MAX_PO2: usize = 21;

/// A wrapper around [InnerReceipt]. It can be deserialized from a Risc0 receipt where it
/// just ignore the journal and metadata fields.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Proof {
    /// The polymorphic [InnerReceipt].
    pub inner: InnerReceipt,
}

impl Proof {
    /// Construct a new Receipt
    pub fn new(inner: InnerReceipt) -> Self {
        Self { inner }
    }

    /// Verify the prove against the given public inputs `pubs` and the vk `image_id`.  
    ///
    /// The context version used is the last `v1_1` that is compatible with all proof generated
    /// with Risc0 1.1.x versions. If you are interested to verify older version see
    /// `verify_with_context` instead.
    ///
    /// Uses the zero-knowledge proof system to verify the seal, and decodes the proven
    /// [ReceiptClaim]. This method additionally ensures that the guest exited with a successful
    /// status code (i.e. `Halted(0)`), the image ID is as expected, and the journal has not been
    /// tampered with.
    ///
    /// - `pubs`: Risc0 `Journal` or just a sha digest of it.
    /// - `image_id`: The Risc0 expected image ID or just a sha digest of it.
    pub fn verify(
        &self,
        image_id: impl Into<Digest>,
        pubs: impl Into<Digest>,
    ) -> Result<(), VerificationError> {
        self.verify_with_context(&VerifierContext::v1_2(), image_id, pubs)
    }

    /// Verify that this receipt proves a successful execution of the zkVM from the given
    /// `image_id`.
    ///
    /// Uses the zero-knowledge proof system to verify the seal, and decodes the proven
    /// [ReceiptClaim]. This method additionally ensures that the guest exited with a successful
    /// status code (i.e. `Halted(0)`), the image ID is as expected, and the journal has not been
    /// tampered with.
    pub fn verify_with_context<SC: CircuitCoreDef, RC: CircuitCoreDef>(
        &self,
        ctx: &VerifierContext<SC, RC>,
        image_id: impl Into<Digest>,
        pubs: impl Into<Digest>,
    ) -> Result<(), VerificationError> {
        log::debug!("Receipt::verify_with_context");
        self.inner.verify_integrity_with_context(ctx)?;

        // Check that the claim on the verified receipt matches what was expected. Since we have
        // constrained all field in the ReceiptClaim, we can directly construct the expected digest
        // and do not need to open the claim digest on the inner receipt.
        let expected_claim = ReceiptClaim::ok(image_id, MaybePruned::Pruned(pubs.into()));
        if expected_claim.digest() != self.inner.claim()?.digest() {
            log::debug!(
                "receipt claim does not match expected claim:\nreceipt: {:#?}\nexpected: {:#?}",
                self.inner.claim()?,
                expected_claim
            );
            return Err(VerificationError::ClaimDigestMismatch {
                expected: expected_claim.digest(),
                received: self.claim()?.digest(),
            });
        }

        Ok(())
    }

    /// Extract the [ReceiptClaim] from this receipt.
    pub fn claim(&self) -> Result<MaybePruned<ReceiptClaim>, VerificationError> {
        self.inner.claim()
    }
}

/// A record of the public commitments for a proven zkVM execution.
///
/// Public outputs, including commitments to important inputs, are written to the journal during
/// zkVM execution. Along with an image ID, it constitutes the statement proven by a given
/// [Receipt]
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub struct Journal {
    /// The raw bytes of the journal.
    pub bytes: Vec<u8>,
}

impl Journal {
    /// Construct a new [Journal].
    pub fn new(bytes: Vec<u8>) -> Self {
        Self { bytes }
    }
}

impl risc0_binfmt::Digestible for Journal {
    fn digest<S: Sha256>(&self) -> Digest {
        *S::hash_bytes(&self.bytes)
    }
}

impl AsRef<[u8]> for Journal {
    fn as_ref(&self) -> &[u8] {
        &self.bytes
    }
}

/// A lower level receipt, containing the cryptographic seal (i.e. zero-knowledge proof) and
/// verification logic for a specific proof system and circuit. All inner receipt types are
/// zero-knowledge proofs of execution for a RISC-V zkVM.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[allow(clippy::large_enum_variant)]
pub enum InnerReceipt {
    /// A non-succinct [CompositeReceipt], made up of one inner receipt per segment.
    Composite(CompositeReceipt),
    /// A [SuccinctReceipt], proving arbitrarily long zkVM computions with a single STARK.
    Succinct(SuccinctReceipt<ReceiptClaim>),
}

impl InnerReceipt {
    /// Verify the integrity of this receipt, ensuring the claim is attested to by the seal.
    pub fn verify_integrity_with_context<SC: CircuitCoreDef, RC: CircuitCoreDef>(
        &self,
        ctx: &VerifierContext<SC, RC>,
    ) -> Result<(), VerificationError> {
        log::debug!("InnerReceipt::verify_integrity_with_context");
        match self {
            Self::Composite(inner) => inner.verify_integrity_with_context(ctx),
            Self::Succinct(inner) => inner.verify_integrity_with_context(ctx),
        }
    }

    /// Returns the [InnerReceipt::Composite] arm.
    pub fn composite(&self) -> Result<&CompositeReceipt, VerificationError> {
        if let Self::Composite(x) = self {
            Ok(x)
        } else {
            Err(VerificationError::ReceiptFormatError)
        }
    }

    /// Returns the [InnerReceipt::Succinct] arm.
    pub fn succinct(&self) -> Result<&SuccinctReceipt<ReceiptClaim>, VerificationError> {
        if let Self::Succinct(x) = self {
            Ok(x)
        } else {
            Err(VerificationError::ReceiptFormatError)
        }
    }

    /// Extract the [ReceiptClaim] from this receipt.
    pub fn claim(&self) -> Result<MaybePruned<ReceiptClaim>, VerificationError> {
        match self {
            Self::Composite(ref inner) => Ok(inner.claim()?.into()),
            Self::Succinct(ref inner) => Ok(inner.claim.clone()),
        }
    }

    /// Return the digest of the verifier parameters struct for the appropriate receipt verifier.
    pub fn verifier_parameters(&self) -> Digest {
        match self {
            Self::Composite(ref inner) => inner.verifier_parameters,
            Self::Succinct(ref inner) => inner.verifier_parameters,
        }
    }
}

/// An enumeration of receipt types similar to [InnerReceipt], but for use in [AssumptionReceipt].
/// Instead of proving only RISC-V execution with [ReceiptClaim], this type can prove any claim
/// implemented by one of its inner types.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub enum InnerAssumptionReceipt {
    /// A non-succinct [CompositeReceipt], made up of one inner receipt per segment and assumption.
    Composite(CompositeReceipt),

    /// A [SuccinctReceipt], proving arbitrarily the claim with a single STARK.
    Succinct(SuccinctReceipt<Unknown>),
}

impl InnerAssumptionReceipt {
    /// Verify the integrity of this receipt, ensuring the claim is attested to by the seal.
    pub fn verify_integrity_with_context<SC: CircuitCoreDef, RC: CircuitCoreDef>(
        &self,
        ctx: &VerifierContext<SC, RC>,
    ) -> Result<(), VerificationError> {
        log::debug!("InnerAssumptionReceipt::verify_integrity_with_context");
        match self {
            Self::Composite(inner) => inner.verify_integrity_with_context(ctx),
            Self::Succinct(inner) => inner.verify_integrity_with_context(ctx),
        }
    }

    /// Returns the [InnerAssumptionReceipt::Composite] arm.
    pub fn composite(&self) -> Result<&CompositeReceipt, VerificationError> {
        if let Self::Composite(x) = self {
            Ok(x)
        } else {
            Err(VerificationError::ReceiptFormatError)
        }
    }

    /// Returns the [InnerAssumptionReceipt::Succinct] arm.
    pub fn succinct(&self) -> Result<&SuccinctReceipt<Unknown>, VerificationError> {
        if let Self::Succinct(x) = self {
            Ok(x)
        } else {
            Err(VerificationError::ReceiptFormatError)
        }
    }

    /// Extract the claim digest from this receipt.
    ///
    /// Note that only the claim digest is available because the claim type may be unknown.
    pub fn claim_digest(&self) -> Result<Digest, VerificationError> {
        match self {
            Self::Composite(ref inner) => Ok(inner.claim()?.digest()),
            Self::Succinct(ref inner) => Ok(inner.claim.digest()),
        }
    }

    /// Return the digest of the verifier parameters struct for the appropriate receipt verifier.
    pub fn verifier_parameters(&self) -> Digest {
        match self {
            Self::Composite(ref inner) => inner.verifier_parameters,
            Self::Succinct(ref inner) => inner.verifier_parameters,
        }
    }
}

impl From<InnerReceipt> for InnerAssumptionReceipt {
    fn from(value: InnerReceipt) -> Self {
        match value {
            InnerReceipt::Composite(x) => InnerAssumptionReceipt::Composite(x),
            InnerReceipt::Succinct(x) => InnerAssumptionReceipt::Succinct(x.into_unknown()),
        }
    }
}

impl From<composite::CompositeReceipt> for InnerReceipt {
    fn from(value: composite::CompositeReceipt) -> Self {
        Self::Composite(value)
    }
}