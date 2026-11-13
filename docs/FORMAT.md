# The S2R2 replay format

This document describes the Heroes of Newerth replay format as implemented by this
crate. It covers S2R2 **v6** and **v7**, the two format versions used by HoN Reborn. The
verified samples are v6 from server 0.13.2.0 and v7 from server 0.13.3.0. The exact patch
where the format changed has not been pinned down.
Everything below was verified bit-exactly against real replays: every frame of the
test replays decodes to its last bit.

All integers are little-endian. "Bits" are read **least significant bit first** (the
K2 engine's `CBufferBit`).

## 1. Container

A `.honreplay` file is a ZIP archive with two members:

| member       | content                                                                 |
|--------------|-------------------------------------------------------------------------|
| `replayinfo` | XML: match id, map, mode, winner, date, length, players (name, account id, team, client number, hero). |
| `replaydata` | The binary S2R2 stream (Zstandard-compressed inside the ZIP on Reborn). |

`replayinfo` uses 0-based `clientnum`; the entity stream uses 1-based client numbers.

## 2. Header

```
char[4]  "S2R2"
u32      format version            (6 or 7 in the verified Reborn samples)
u8[4]    server version            (e.g. 0.13.3.0)
2 x typedesc table                 (see 2.1)
u32      type map count, then count x { u16 entity type, u16 base typedesc id }
cstring  map name
u32      string set count N, then N-1 x cstring string set (see 2.2)
u32      state block count M, then M-1 x { u32 length, bytes }
```

### 2.1 Typedescs (entity schemas)

```
u32 count
count x {
  u16     type id
  cstring name                      e.g. "Player", "Hero", "Item"
  u32     flags
  u8      field count
  field count x { cstring name, u8 type code, u32 bits, u32 arg }
  u16     default snapshot length, bytes   (an entity record, see 4, holding default values)
}
```

Table 0 holds **static engine types** addressed directly by a type id below 256
(Player, TeamInfo, GameStats, Shop_Info, Entity_* ...). Table 1 holds the **base classes
of dynamic game entities** (Hero, Ability, Item, Gadget, Creep, Building, GameInfo ...).
Dynamic entity types (one per `.entity` definition file, ids >= 256) map to a table-1
base class through the type map.

### 2.2 String sets

Serialized as `key 0xFF value 0xFF key 0xFF value ...`.

| set | content |
|-----|---------|
| 0 | server cvars (`svr_gameFPS`, `svr_name`, ...) |
| 1 | game cvars |
| 2 | dynamic strings: server date/time, game name, team names, player names, clan names, clan ranks (indexed by `Player.m_unNameIndex` etc.) |
| 3 | resource paths keyed by resource id (`064/heroes/parasite/base/hero.entity`, effects, ...) |

Frames can replace a whole string set (see 6).

### 2.3 State blocks

* **Block 0**: array of `{u16, u16}` pairs. Its length defines how many 32-bit state
  groups each frame carries (`ceil(len/4/32)`); the meaning of the pairs is not needed
  for decoding.
* **Block 1**: entity map, array of `{u16 entity type, u32 resource id}` giving the
  `.entity` path of every dynamic type through string set 3.

## 3. Frames

After the header the file is a sequence of frames:

```
u32   body length, body bytes        (bit stream, section 4)
2 x { u32 count, count x { u32 receiver client, u32 length, bytes } }   packet queues
u32   string updates, n x { u32 set index (1-based), cstring string set }
u32   commands, n x { u32 kind, u32 length, bytes }                     (format > 3)
```

The server runs at `svr_gameFPS` (100 on Reborn); one frame per tick. Frames recorded
during a pause have a clock delta of 0.

### 3.1 Frame body

```
u32      sequence
prev     1 => sequence-1 | 01 x4 => sequence + ~x | 00 x32 => explicit
u32      server clock (ms)
u8       reserved (always 0)
u8       game event count
events   (see 5)
mask     state group mask (compressed mask, count = state groups), then 32 bits per set group
records  entity records until the end of the body (see 4)
```

## 4. Entity records

```
record  := 0                     implicit: next *active* entity id, then body
         | 1 delta has_type [type:16 [variant]] active [variant] body

delta   := 1                 -> +1
         | 0 1 x:4           -> 1 + x
         | 0 0 1 x:8         -> 0x11 + x
         | 0 0 0 x:15        -> 0x111 + x
```

* The id cursor starts at 0 each frame and only moves forward.
* `has_type = 1, type = 0` deletes the entity. `type != 0` creates it (or re-creates it
  with a new type, e.g. after an alt-avatar swap); its body is a diff against the type's
  default snapshot.
* `active` is a persistent per-entity flag. Implicit records visit the next entity with
  the flag set, so frequently updated entities cost a single bit of addressing.
* A frame ends at the end of its body; up to 7 trailing zero bits are padding.

### 4.1 Variant bits (dynamic types only, type >= 256)

Created dynamic entities carry the cosmetic **variant** (alt avatar, alternative item
model ...) of their definition:

* **v7**: before `active`: `1 bit present`, and if set a **5-bit variant key**.
* **v6**: after `active`: a fixed number of bits per entity type equal to
  `ceil(log2(number of variants))`. The count is not stored in the replay; the decoder
  predicts it from the resource paths (`.../<name>/base/...` vs sibling variant
  directories) and verifies the choice by requiring the rest of the frame to decode to
  its exact end.

### 4.2 Body: field mask

The body starts with a compressed bit set of changed fields:

* `count <= 8`: `count` raw bits.
* otherwise: 1 bit "any field set"; then a binary tree over `2^ceil(log2(count))`
  leaves, walked level by level. For every node whose parent is set:
  `0` = only the left child, `1 0` = only the right child, `1 1` = both.
  The leaves are the field flags. A set leaf beyond `count` means the stream is corrupt.

### 4.3 Body: field values

Changed fields follow in declaration order. Width = the field's `bits`, or the type
default:

| code | meaning | default bits | notes |
|-----:|---------|-------------:|-------|
| 0 | byte / small int | 8 | |
| 1 | u16 | 16 | string indices, hero ids |
| 2 | u32 | 32 | times (ms, `0xFFFFFFFF` = none), counters |
| 3 | f32 | 32 | |
| 4 | u64 | 64 | |
| 5 | vec3 f32 | 96 | |
| 6 | entity index | 16 | `0xFFFF` = none |
| 7 | resource handle | 16 | |
| 8 | angle | 8 | `value * 360 / 2^bits` degrees |
| 9, 10 | u16 | 16 | |
| 11, 12 | rounded float | 16 | health, mana, experience (usually 15 bits) |
| 13 | byte fraction | 8 | `value / 255` |
| 14 | vec3 u16 | 48 | world position of static units |
| 15, 16, 17 | delta-coded int vector, 1/2/3 components | - | each component: `1 x5` / `0 1 x6+32` / `0 0 x15`, zig-zag signed. Absolute on create, **added** on update. Used for positions. |
| 18 | 128-bit | 128 | account ids (UUID on Reborn), guids |

## 5. Game events

```
u32 mask
payload, in this order for each set bit:
  bit 0   4 bytes
  bit 1   8 bytes   source entity (u16) + source position (3 x u16)
  bit 2   6 bytes   source position
  bit 3   3 bytes
  bit 4   2 bytes
  bit 5   8 bytes   target entity (u16) + target position (3 x u16)
  bit 6   6 bytes   target position
  bit 7   3 bytes
  bit 8   2 bytes
  bit 9   3 bytes   effect resource id (u16, string set 3) + 1 byte
  bit 10  2 bytes
  bit 11  2 bytes
  bit 12  2 bytes
  bit 15  2 bytes
  bits 16, 17 flags without payload
```

Events are byte sized but not byte aligned inside the bit stream. The legacy 16-bit-mask
layout used by older community parsers is wrong for Reborn; it desynchronises every
frame that contains an event.

## 6. Packets

The packet queues hold the reliable messages each client received that frame
(`receiver` = 0-based client index as in `replayinfo`, `0xFFFFFFFF` = broadcast). Several messages can be
concatenated. Known message ids:

| first byte | message |
|-----------:|---------|
| `0x02` | all chat: `02 <sender> <utf-8 text> 00`, sender = 1-based client number (`Player.m_iClientNumber`) |
| `0x03` | team chat |
| `0x0D` | localized server notice (e.g. `rejected_invalid_ward_location`) |
| others | UI, sound, popups; preserved raw (`Frame::packets`) |

Chat is sent to every recipient, so the analysis layer de-duplicates by
(time, sender, text).

## 7. Time

`Frame::clock` is the server clock in ms. `GameInfo` holds the game phase:

| phase | meaning |
|------:|---------|
| 3 | pre-game (heroes picked, `m_uiPhaseDuration` = countdown) |
| 4 | match running, `m_uiPhaseStartTime` = game time 0 |
| 5/6 | game over / post-game |

Game time = `clock - start of phase 4`. This equals the engine's own game-time values
(for example `Player.m_uiLastDeathTime`) exactly. `GameInfo.m_uiPauseTime` accumulates
paused time. Hero picks and bans happen before recording starts.
