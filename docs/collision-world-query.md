# Collision world query: the per-bucket segment query (005b2630)

Scope: the questions in the brief (tree layout, mask test, args 6/7, other rejections, builders). Every statement is tagged. Addresses are the functions read. Pseudocode lines are the `fn.py 005b2630 --lines` numbering.

## 0. What was read

- [game] 005b2630 `query_segment_lists` (full pseudocode, lines 1-185). Ledger line written.
- [game] 005b3a40 (append helper, outline; the body is 10 lines). Ledger not written (the brief allows only the read function to be noted; see section 6).
- [game] 005b0f30 `segment_box_clip` (full pseudocode, lines 1-92). Ledger line written.
- Callers of 005b2630: only 005b3620 [game, `fn.py callers`].
- Not read: 005b3620, 005b37f0, 005b3590. Their facts below come from the brief, not from this pass.

## 1. The structure is not a tree

[game] 005b2630 does not walk nodes with children. Its `param_1` is one bucket object. The object holds three per-axis lists:

- X list head pointer at object+0x34, Y at +0x40, Z at +0x4c (pseudocode lines 13, 39, 44; the three-pointer stride is 0xc, lines 13-37).
- Each list is an array of 8-byte nodes, linked by index. Node layout as the code uses it:
  - +0 u16 key. Bit 0 of the key is the endpoint type: 0 = lower bound (entry), 1 = upper bound (exit) (line 49, 111).
  - +2 u16 index. Used as `index*8` for the node address and `index>>1` for the record number (lines 50, 56, 119, 127). So index = 2*record + type.
  - +6 u16 next index. `0xffff` ends the chain (lines 19-24, 40-45, 154-156). Each list has a head at the array base; its +6 is the first node's index (line 18-23).
- Lower and upper bounds of one record on an axis are the two nodes at indices 2r and 2r+1 of that axis array (line 51: `X + 8 + idx*8` is the partner of `X + idx*8`).
- Records are a separate table at object+0x28, stride 0x1c, indexed by `index>>1` (line 57, 58, 127).

So a bucket object is three sorted endpoint lists (a sweep-and-prune structure, one list per axis) plus a record table. The brief's word "tree" does not fit. [game] for the layout; [assumed] that the lists are sorted by key, since the walks stop at the first key above a bound (lines 26, 47, 77) and the segment walk advances in key order (lines 110-182).

Record fields used (record = object+0x28 + r*0x1c):
- +0x4 holder pointer (copied into the out list, lines 64, 134, 142).
- +0x8 u32 flags. Bit 0x10 must be 0 (lines 63, 133).
- +0x10 int. Compared with param_6 and param_7 (lines 62, 132).
- +0x18 u16 mask A. Tested against param_4 (lines 57-59, 128-129).
- +0x1a u16 mask B. Tested against param_5 (lines 60-61, 130-131).

## 2. Q1: the traversal (tree question, answered as lists)

[game] The function does two passes:

- Pass 1 (lines 39-78): walks the Z list over lower-bound nodes whose key is <= the start point's Z (line 47, 77). For each, it tests the record's box against the start point: X hi and Y hi and Z hi >= start, X lo and Y lo <= start (lines 51-55, Z lo is implied by the list order). Survivors go through the Section 3 tests.
- Pass 2 (lines 79-182): a sweep from the start to the end. Per axis it computes the parametric time of the next endpoint, `t = (key - start) * (2^24 / delta)` (lines 83-93, 0x1000000 = 1.0 in 24-bit fixed point). It repeatedly takes the axis with the smallest t (lines 98-108, 173-180) while t < 1.0 (line 110). At each endpoint event on an entering face (lower bound moving +, or upper bound moving -, lines 111-116) it tests the record with the cursor point (current segment position, held in `local_40`) inside the record's box on all three axes (lines 121-126). Survivors go through the Section 3 tests and are appended inline (lines 127-143). The cursor is then advanced to that axis's next node (lines 148-177).

Traversal order: pass 1 first (boxes containing the start, Z-list order), then pass 2 (boxes entered by the segment, in increasing t along the segment).

## 3. Q2: the mask test (exact)

[game] Mask A (record +0x18, u16) against param_4 (query mask, world+0x26c per the brief). Pseudocode line 59 (pass 1) and 128-129 (pass 2):

    ((A == 0) && (param_4 & 0xffff) == 0xffff) || ((A & param_4) != 0)

[game] Mask B (record +0x1a, u16) against param_5. Lines 60-61 and 130-131:

    ((B == 0) && (param_5 & 0xffff) == 0xffff) || ((B & param_5) != 0)

Consequences:
- With param_4 = 0xffff (the "match everything" value), the A test passes for every record (A == 0 or A != 0). With a real mask, a record passes when A has a bit in common with the mask; a record with A == 0 passes only the all-ones query.
- With param_5 = 0xffff (the caller passes 0xffff, per the brief), the B test always passes. B is effectively unused by this call.

So: a body is a candidate when the record's A (+0x18) shares a bit with the query mask, or A is zero and the query mask is 0xffff. [game]

Send or receive: NOT SETTLED. The record's +0x18 and +0x1a are copies of something; the brief names body fields +0x9c and +0x9e as the two masks. This function does not show which body field the record copies. The loader (docs/collision-loader.md, section 3) reads `sendMask` at 005ba394 and `receiveMask` at 005ba3b9 on the batch primitive; which of these lands in record +0x18 is not settled (see Q5).

## 4. Q3: args 6 and 7 (world+0x274, +0x278)

[game] Pseudocode line 62 (pass 1) and 132 (pass 2): a record is skipped when its +0x10 equals param_6 OR param_7. Both are compared as 32-bit values with `!=` (`*(int*)(rec+0x10) != param_6 && != param_7`).

So args 6 and 7 are two pointers (or two ids) of bodies to skip: the "exclude these two holders" test, typically the querying body and its owner or parent. [game] for the comparison. [assumed] that +0x10 is a holder pointer (it is compared to the same kind of value and the caller passes world+0x274 / +0x278 values); not settled what the caller stores there. They are not callbacks: nothing in 005b2630 calls through them.

## 5. Q4: other rejections before a body is appended

[game] In order, a record reaches the out list only if all of these pass:

1. The segment clip (005b0f30) did not miss the bucket box. If it returns 0 (line 12 of the 005b2630 pseudocode), no node is visited. 005b0f30 zeroes both outputs on a miss and returns 0 (005b0f30 lines 54-60).
2. Pass 1 only: the lower-bound node was reached by the Z-list walk with key <= start Z (lines 47, 76-77).
3. Pass 2 only: the event is an entering face for that axis, from the direction of the segment (lines 111-116). Exits are ignored.
4. Box containment: start point (pass 1, lines 51-55) or cursor point (pass 2, lines 121-126) inside the record's box on all three axes.
5. Mask A (Section 3).
6. Mask B (Section 3).
7. Not equal to param_6 or param_7 (Section 4).
8. Bit 0x10 of the flags at +0x8 is clear (lines 63, 133). This is the "0x10 bit" the brief asks for: a set bit rejects the record. [game] for the test. The meaning of the bit is not settled.

After these, the holder pointer at +0x4 is appended: pass 1 through 005b3a40 (`list[count++] = holder` with growth via 00578830 when full, min 0x10, lines 65 and 005b3a40), pass 2 inline with the same growth (lines 135-143). The list is an int* object: [0] data, [1] count, [2] capacity.

There is no separate AABB test against the segment beyond the sweep and the cursor box test. [game]

## 6. Q5: who builds the buckets and the trees

NOT FOUND in this pass. Checked:
- `decomp_lookup.py grep "\+ 0x134"` and `"0x1d4"`: about 20 hits each, all large unrelated functions (e.g. 004618f0, 008ab960, 006e2c10). Not read; the grep is too broad to name a builder.
- `decomp_lookup.py grep "\* 0x1c"`: many hits, unrelated.
- the unpublished notes: the soup builder 005b8fa0 builds a flat list with no spatial index. It is not the bucket builder. [game]
- docs/collision-loader.md section 3: the loader 005b9250 reads body and batch fields `sendMask` (005ba394), `receiveMask` (005ba3b9), `material`, `maskPreset` (005ba2f4). This is the probable source of the body masks that end up in records, but no write of record +0x18 or +0x1a was found. Not settled.

Candidate writers not read (the brief said at most two; none read): the function that stores into object+0x34 / +0x40 / +0x4c and the world's +0x74 / +0x134 boxes. Nothing decides in this pass which of the 16 buckets a body goes into. NOT SETTLED.

## 7. Assumptions added (to be logged in the unpublished notes by the lead)

- [assumed] The per-axis lists are sorted by key ascending (walk stops at first key above a bound). Settled by reading the builder.
- [assumed] Record +0x10 is a holder pointer compared with the caller's body ids. Settled by reading the caller and the builder.
- [assumed] Bit 0x10 of record +0x8 means "not queryable" or similar. Meaning not settled.
- [assumed] Record +0x18 / +0x1a are copies of body masks (sendMask / receiveMask). Which one is +0x18 is not settled.

## 8. Rule check

No hard rule was broken: no decomp file opened directly, no cd, no edit outside the notes file and the three ledger lines, no game file touched, no build or run. Two ledger lines were written (005b2630, 005b0f30).
