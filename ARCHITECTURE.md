# nomos-engine architecture

The daemon owns a versioned embedded Sema database at the configured
`nomos.sema` path. One component record family contains Capsule rows and slot
rows, allowing a fresh Capsule assertion and live-slot mutation to share one
atomic commit. Fresh databases contain no Capsule or slot rows.

Capsule rows retain the immutable canonical sealed archive and an append-only
sequence of authenticated NameTree projections. Slot rows retain only full
Nomos Capsule identities, the seated set, live binding, generation, ordered
generation-class metadata, and the binding commit marker. Short hexadecimal
forms are resolved ephemerally inside one slot and are never persisted.

Deploy validates the full Capsule/projection relationship before state access.
An exact current identity, exact current projection, and identical ordered
selection returns `AlreadyCurrent` without entering a write API, even when the
supplied CAS expectation is stale. Any binding change is one atomic transaction
and advances the slot generation once. The binding marker is the overflow-
checked predicted next database marker embedded in that same write; the atomic
receipt must match both marker and operation count. The daemon serializes all
operations through one runtime mutex. Any impossible post-commit divergence
poisons the process-local engine until restart and recovery; no second
marker-stamping write exists.

Projection advancement is a separate Capsule-row mutation. It appends the exact
successor projection without changing any slot generation or deployment
metadata. Until a public translator rename receipt verifier exists, a supplied
opaque receipt is explicitly unsupported and the operation requires the
configured admin Unix peer UID.

Bootstrap transformation admits either the live binding or one retained seat
and snapshots the full Capsule identity, slot generation, and projection
version. Its input is a production `VerifiedBootstrapAssembly`: the matching
reader revalidates the exact authority receipt and complete prepared model, then
`BootstrapSliceOneLowering` consumes the branded transaction directly. No
WholeEthos value, six-slot adapter, draft reconstruction, or identity minting
enters this path.

Prepared bootstrap transactions are explicitly `NotYetArchived`. The current
opaque `Request::Transform` wire therefore returns the typed
`EthosPopulationInvalid` refusal rather than inventing an archive for source and
authority approval. The in-process result is complete `WholeLogos`; its bytes
are restored through `WholeLogos::from_archive_bytes` before a reply is
constructed. There is no `MacroPackage`, native WholeEthos evaluator, fixture,
central storage socket, or output-slot write in the live daemon path.

Slot identity, CAS policy, ordered generation metadata, database schema version,
admin UID policy, and the native dual output carrier remain
`[to-be-reviewed-by-psyche]`; po2.8 owns retiring the temporary generation-class
selection.
