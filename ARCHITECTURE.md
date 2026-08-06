# Architecture — nomos-engine

## Authority-sealed transformation

The live engine is a library boundary over one complete
`VerifiedBootstrapAssembly`. The assembly carries the reader and the
authority-branded transaction produced by Sema Translator:

```text
VerifiedBootstrapAssembly
          │
          v
AuthoritySealedBootstrapTransformation::lower_bootstrap
          │
          ├─ Interface or Nexus → BootstrapSliceOneLowering::lower
          └─ Sema               → BootstrapSliceOneLowering::lower_sema
          │
          v
BootstrapTransformationOutcome { WholeLogos, archive }
```

Core Nomos revalidates the exact authority receipt and complete prepared model
before lowering. The engine cannot accept a draft, substitute a reader, allocate
an identity, or reconstruct another Ethos representation.

## Storage provenance

Sema lowering receives explicit `ExternalStorageProvenance` for every nonlocal
stored type. Core Nomos validates each complete identity, structural storage
fingerprint, and owning published source revision. Interface and Nexus lowering
reject any supplied external storage evidence because those file kinds have no
storage-evidence position.

## Result boundary

Successful lowering produces typed `WholeLogos`, archives that exact value, and
restores the bytes through `WholeLogos::from_archive_bytes`. The engine returns a
`BootstrapTransformationOutcome` only when the restored value equals the typed
result. Lowering errors, archive errors, unexpected storage evidence, and an
archive round-trip mismatch remain distinct typed refusals.

`tests/bootstrap.rs` proves successful Nexus and storage-aware Sema lowering,
strict external-evidence admission, authority validation through the assembled
transaction, and equality of the returned typed and archived results.
