# Native subset v9 — per-view authoring settings

This extends the implemented subset documented in v4 and the host contracts for
v5 interrupted endings, v6 selection bookmarks, v7 chosen stroke appearance and
v8 canvas layers. It remains short of the full approved interchange contract.
Current writers emit native `version:9`; readers accept v1–v8 with the documented
absence/default rules. The checked/encrypted envelope and other top-level fields do
not change. Unknown/unsafe versions or fields fail closed.

Each object in `attachments.views` may have `authoring`. An absent/null authoring
value means no stored preset/background configuration. Writers omit None, keeping
legacy view serialization unchanged. Native versions below 9 cannot carry Some
authoring metadata. Old basic pen width/colour/camera fields remain meaningful.

```json
{
  "authoring": {
    "presets": [{
      "id": "00000000-0000-4000-8000-000000000001",
      "name": "Fine blue",
      "width_mm": 0.6,
      "colour_rgba": [17, 128, 255, 255]
    }],
    "selected_preset": "00000000-0000-4000-8000-000000000001",
    "background": {
      "kind": "Graph",
      "spacing_mm": 5.0,
      "line_width_mm": 0.2,
      "origin_mm": [0.0, 0.0],
      "colour_rgba": [100, 100, 100, 128],
      "paper_rgba": [255, 255, 255, 255],
      "include_in_export": false
    }
  }
}
```

`presets` is ordered, at most 32. IDs are UUIDs unique **within that view**; they
are not global asset identities and remain unchanged when their containing view is
copied/remapped. Names are nonblank, at most 256 UTF-8 bytes. Widths are finite,
greater than zero and at most 100 mm. RGBA is exactly four integers in [0,255].
`selected_preset` is null or references a preset in this object; if selected, its
width/colour must exactly match the containing view's current `pen_width_mm` and
`colour_rgba`. Clearing the marker leaves the current custom pen appearance intact.
Readers must reject dangling selections, duplicate IDs or invalid values; there is
no implicit first-preset selection or clamping.

`background` is null or the complete object above. `kind` is one of Blank, Ruled,
Dotted or Graph. Spacing is finite in (0,1000] mm; line width is finite, positive
and no greater than spacing. The two origin components are finite millimetres in
the host's declared source frame. Colours follow the same RGBA rules. An export
choice excluding backgrounds must omit both guides and paper fill. Absence must
not be interpreted as an invented background or a newly chosen pen style.

These are per-view preferences, subject to the view's current owner/exercise/
anchor/bookmark access rules. They do not mutate source coordinates, raw pressure,
accepted content, object relations or existing stroke appearance. Preference changes
do not consume document edit Undo. A host must explicitly calibrate physical widths
into its named source units for future capture and snapshot the resulting style at
contact begin; device DPI/accuracy is not established by this format. Pressure response
and eraser behavior are not defined by this extension.

Whole-library snapshot schema 15 marks support for these optional view fields. Its
`view_settings` rows remain [id, entity, payload]; there are no new tables or rewritten
legacy payloads in migration 015. Current restore reads schema 8–14 without inventing
authoring metadata, and rejects populated authoring under older tags. View-local
preset references remain local in native copies; global view/entity references still
remap according to their existing contracts. Model files/provider grants remain outside
this format. No raw-source/receipt encoding changes are implied by v9.
