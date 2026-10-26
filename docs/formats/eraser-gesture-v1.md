# Eraser gesture command, version 1

The existing host request version 1 accepts an additional authoring command:

```json
{"version":1,"command":{"op":"EraseGesture","block":"<UUID>","revision":0,"gesture":{"coordinate_space":"declared-source-space","path":[[1.0,2.0],[4.0,2.0]],"radius":0.5}}}
```

`radius` is a positive finite distance from the observed centreline in the declared source units, not a device-pixel or physical-millimetre guess. Coordinates must be finite; the path must contain at least one point. A single point is a circular whole-stroke selector. The polyline sweeps that circle between successive points. Pen and explicitly captured finger strokes can be targets; raw eraser observations are not targets. Neither pressure nor chosen stroke width expands this centreline selector.

The result is the current block. A hit changes only visibility through the existing revisioned, atomic Undo operation. A miss returns the original revision. Original samples remain available through the independently authorised source API. Stale revision, denied access, mismatched source space, resource admission or unrepresentable geometry fail without a partial edit. See ADR 013 for admission limits and remaining gesture/scene work. No persistent schema change is needed.

## Compound operation

An additive host version-1 command accepts several targets:

```json
{"version":1,"command":{"op":"EraseGestures","targets":[{"block":"<UUID>","revision":0,"gesture":{"coordinate_space":"declared-source-space","path":[[1.0,2.0]],"radius":0.5}}]}}
```

The result is an array of blocks in request order. All targets must belong to the same master note/canvas and have distinct block IDs. All hit sets commit atomically as one Undo operation; a miss retains that block's revision. If every target misses, the library generation and Undo remain unchanged. Work admission is shared across the whole request. See ADR 014 for the complete scope and limits. The singular `EraseGesture` response remains a single block.
