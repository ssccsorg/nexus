# Integration seam

neXus records a fact, an intent, and a hint over one coordinate space. An external claim or identity system attaches at
one place: the three things a fact says about its own provenance, which are its content, its origin, and its creator. The
core does not learn the external protocol, and the adapter does not change the record.

## The seam

| What the external system holds | What it becomes |
|---|---|
| a payload | the record's content |
| an issuer or subject identifier | the record's origin and creator |
| a proof over the payload | the record's content hash and coordinate |
| an evaluation or decision | an intent and its conclusion |

The proof is the row worth reading. A record's coordinate follows from what the record holds, so a verifier that holds the
record recomputes the coordinate and compares. The external proof needs no copy inside the record, because the record is
its own check.

## Writing the adapter

1. Map the payload to content and store it. The content hash is the payload's identity.
2. Map the issuer to origin and creator. Both are part of the address, so two issuers publishing the same payload keep two
   records.
3. Map the decision to an intent and its conclusion. The intent lifecycle then records when the work was proposed, who
   claimed it, and when it concluded.

Nothing else is needed. A field the external system keeps only for its own search can become a coordinate axis, and the
same question is then asked of the record layer instead of the external system.

## Where the adapter lives

Outside the core. In this repository it would be a downstream crate, and in a product tree it would be the product's
integration. The rule is that the core depends on no external system, and an external system reaches the core through this
seam alone.

## What is not included

- No external chain, token, or registry.
- No identity provider.
- No production SLA.
- No second storage format.
