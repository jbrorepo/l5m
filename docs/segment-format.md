# L5M Segment Format

All integer fields are little-endian. Version `1` segments begin with a 128-byte header followed by fixed-width capsule metadata, a string payload area, a relation area, and an index summary area.

## Header

Offset | Field
--- | ---
0 | Magic bytes `L5MSEG01`
8 | `u32` version
12 | `u32` header length, currently `128`
16 | `u64` epoch
24 | `u64` tenant ID
32 | `u64` capsule count
40 | `u64` metadata table offset
48 | `u64` string payload offset
56 | `u64` relation area offset
64 | `u64` index area offset
72 | 32-byte BLAKE3 segment hash
104 | reserved bytes

The segment hash is computed over the whole file with the hash field zeroed.

## Capsule Metadata Table

Each metadata record is 340 bytes:

- `u128` capsule ID.
- `u64` tenant ID.
- `u64` source ID.
- 32-byte source hash.
- Four `u64` semantic bit words.
- 64 signed residual bytes.
- `i64` valid from.
- `i64` valid until, with `i64::MAX` representing none.
- `i64` observed at.
- `i64` last verified at.
- `u128` context mask.
- `u128` policy mask.
- `u8` trust level.
- `u8` classification.
- `u8` poison risk.
- Padding byte.
- Five string/list blob references for claim, evidence, source URI, anchors, and entities.
- Relation offset and relation count.
- 32-byte content hash.
- Reserved bytes.

String blob references are `u64 offset` plus `u32 length`, relative to the string payload area. A source URI of `u64::MAX/u32::MAX` means absent.

## String Payload Area

Claims, evidence, and source URIs are UTF-8 blobs. Anchor and entity lists are encoded as `u32 count`, then repeated `u32 byte_length` plus UTF-8 bytes.

## Relation Area

Each relation record is 37 bytes:

- `u128 from`.
- `u128 to`.
- `u8 kind`.
- `i16 weight`.
- Two reserved bytes.

Relation kind values are ordered as supports, contradicts, supersedes, depends on, derived from, duplicate of.

## Indexes

The compiler writes an index summary marker, and the loader builds runtime indexes from the binary segment:

- Anchor hash to sorted capsule ordinals.
- Entity hash to sorted capsule ordinals.
- Semantic bucket to sorted capsule ordinals.
- Capsule ID to ordinal.

Hashes use BLAKE3 folded to `u64`. The semantic bucket is currently the low 16 bits of the first semantic word.

## Compatibility

Readers must reject unknown magic, unsupported versions, unsupported header lengths, out-of-bounds offsets, invalid UTF-8 payload strings, invalid relation kind bytes, and segment hash mismatches. Future formats should increment the version and preserve little-endian encoding.
