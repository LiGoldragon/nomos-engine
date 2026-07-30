# Agent guidance

This repository is under fast development and constantly breaking.

The daemon is stateful and owns its embedded, versioned `nomos.sema` database.
A separate small translator daemon is authoritative only for shared naming and
encodedID allocation.

Do not reintroduce central storage, actor-plane indirection, fixture packages,
legacy `MacroPackage` evaluation, or daemon-side Logos output writes.
