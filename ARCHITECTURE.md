# nomos-engine architecture

A real daemon and thin CLI. In the currently wired legacy topology,
Signal → Nexus → SEMA Kameo actors fetch TypeSchema Core from the central
socket, apply the typed `MacroPackage::wire_fixture`, and persist package
identity and resulting CoreLogos through typed binary messages.

The approved target is a stateful Nomos daemon whose own embedded sema
database stores its state. A separate small translator daemon owns only shared
naming and encodedID allocation. The current socket path remains in place
until a separately designed storage migration lands; it is not the target
storage contract.
