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

Transform admits either the live binding or one retained seat and snapshots the
full Capsule identity, slot generation, and projection version. The input is a
canonical `EncodedPopulation<WholeEthos, EngineEthosNameTree>` archive. Its
WholeEthos value, tuple invariants, complete item/variant declaration closure,
derived-name realization plan, exact Universal-to-Rust reference mappings, and
expected Logos declaration/reference closures are checked before evaluator
entry.

`NativeAuthoredEvaluator` consumes the sealed authored declarations directly.
There is no `MacroPackage`, fixture, enriched evaluator, central storage socket,
or output-slot write. The returned native Logos population carries its complete
declaration/reference closure. Before a reply is constructed, its bytes are
canonically restored through `NativeLogosPopulation::from_archive_bytes` and
rebound to the authenticated input plan.

The exact ancestor and reachable-spelling closure of a complete NameTree
remains po2.4 work and is not claimed by this slice. Slot identity, CAS policy,
ordered generation metadata, database schema version, admin UID policy, and the
native dual output carrier remain `[to-be-reviewed-by-psyche]`; po2.8 owns
retiring the temporary generation-class selection.
