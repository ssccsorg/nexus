# Integration seam

An external claim or identity system attaches to the record layer at one place: the record's content, origin, and creator. Nothing in the core needs to know the external protocol.

| External claim system | neXus |
|---|---|
| Claim payload | Record content |
| DID or issuer | Origin and creator |
| Claim proof | Content hash and coordinate |
| Oracle evaluation | Intent and conclusion |

The adapter lives outside the core. In this repository it would be a downstream crate, and in a product tree it would be a product integration. The core keeps the storage contract and does not depend on the external system.

## Example: IXO

IXO's impact credits carry Entity, Claims, Location, Period, Oracle, Protocol, Data, and Proof. A thin adapter can map those fields as follows.

- Entity and Oracle become origin and creator.
- Claims and Data become the record content.
- Location and Period become query dimensions in the adapter.
- Proof becomes the content hash and the coordinate.

This is a mapping, not an integration. No IXO chain, token, or portal is part of this crate.

## What is not included

- No external chain or token.
- No identity registry.
- No production SLA.
- No second storage format.
