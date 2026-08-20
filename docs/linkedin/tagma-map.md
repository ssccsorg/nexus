# LinkedIn post draft: Tagma Map

Draft for a public post about the Tagma Map primitive and the layered
restructure it prompted in nexus. Written in English.

---

Title: The lesson from storing 256-bit hashes in a coordinate tree

Body:

Tagma Map is a hash-less, collision-free coordinate address space. Its
claim is simple: every key is a path of coordinates, no hashing, no
collision resolution. In an injective encoding, a 256-bit SHA-256 value
maps to exactly one 20-coordinate path, so two distinct contents can
never share an address.

We put that claim to work in nexus, our FIH knowledge store, and the
measurements changed how we think about the primitive.

The first design stored everything in one deep coordinate tree:
timestamps, origins, creators, record identities, and the full content
hash. It was elegant and wrong. The identity and hash coordinates are
pseudo-random, so every record carved its own branch through the tree,
about thirteen 89 KB nodes per record. Memory grew at roughly 3.4 MB
per fact. The hash axes were the worst offender: SHA-256 has no
similarity structure, so those coordinates could never help a spatial
query. They existed only to let same-id records coexist, which the
application layer could handle better.

The restructure was a lesson in layered systems. The tree now holds
only low-cardinality structural axes (time, entity, origin, creator,
status) and maps each path to the set of record ids there. Record
bodies, content identity, and relationships live in HashMap record
maps, the way a database separates an index from the heap. Memory is
bounded by axis cardinality, not record count.

The numbers: about 3.4 MB per fact before, under a kilobyte per fact
after, with a fixed structural index that does not grow with the data.
The collision guarantee moved from a ~40-bit birthday bound to the full
256-bit space of SHA-256.

Two takeaways. One: coordinate trees are a narrow, precise tool. They
are exceptional for low-cardinality axes and for injective content
addressing, and punishing for pseudo-random keys in a deep tree. Two:
the collision-free promise is only as good as the encoding. Fold a hash
into six coordinates and you keep forty bits. Encode it injectively
into twenty and you keep all 256.

---

Notes for the author

- The 3.4 MB and sub-kilobyte figures come from the nexus memory probe
  (`nex/process/tests/memory_probe.rs`), 16 facts, counting allocator.
- "FIH" (Fact-Intent-Hint) may need a one-line gloss for a general
  audience: facts are immutable content, intents are operations over
  them, hints are constraints.
- Keep the "elegant and wrong" line only if the tone fits the audience.
