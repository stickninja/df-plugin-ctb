# df-plugin-ctb

DragonFruit Plugin for the CTB Encoder

## Current status

- Plugin role: **encoder-only** (no network/runtime protocol surface).
- Output target: `.ctb`
- Encoder path: raw raster mask layers from `dragonfruit-slicer-v3` (PNG path disabled).
- Implementation stage: concrete CTB binary serialization enabled (real CTB magic/header, print/slicer tables, layer definitions, CTB RLE packets).

## Resolved layer timing contract

CTB v4/v5 (plain and encrypted) optionally consume `ctb.layerPlanV1` from the
slicing job metadata. Version 1 carries `modelLayerCount`, `startupDummy`,
`normalDefaults`, `bottomDefaults`, and resolved `layers` in file order. Each
record has a one-based `modelLayerNumber` (null for the dummy), `isDummy`,
physical `positionZMm`, exposure, independent light-off delay and three waits,
stage motion values, and raw 0–255 `pwm`. The two defaults objects carry the
four timing values for global headers, independently of layer overrides.

The encoder validates counts, identity, physical Z and finite nonnegative
timing/motion, rejects unsupported versions, and activates per-layer settings.
A missing plan uses the existing encoder behavior. Model rasterization retains
its original layer count and physical Z. Startup insertion occurs during CTB
assembly: one intensity-128 pixel at the first real image's first illuminated
pixel (image-center fallback), same Z as the first real layer, and rekeyed CTB
image payloads for the shifted file indexes. A positive bottom count increases
by one; zero remains zero. The resolved dummy uses 0.01-second exposure, PWM 1,
0.1-mm total lift and zero delays. Physical print height and all real layer
records are preserved. The time estimate is exposure plus explicit waits plus
the larger of raw light-off delay and calculated motion time, without
acceleration or firmware compensation.

`read_ctb_layer_settings_from_file` and its bytes counterpart read actual
stored v4/v5 layer records, including encrypted-file settings. They do not
reconstruct timing from a profile or infer dummy identity. Their result exposes
file layer count, bottom count, per-layer activation, Z, timing and motion for
the preview inspector.

Focused Rust tests cover plain/encrypted v4/v5, independent timing, Simple
motion, dummy image decoding and physical Z, invalid plans, global timing
defaults, and print-time estimates. Set `DF_CTB_FIXTURE_DIR` while running the
tests to save synthetic CTB fixtures for an independent decoder check.

## Legal notice (interoperability)

This plugin includes format-compatibility work for CTB-family resin files to enable interoperability between software ecosystems.

The project is developed in good faith for compatibility use cases, with attention to applicable legal frameworks such as:

- EU Directive 2009/24/EC (interoperability-related reverse engineering allowances)
- DMCA Section 1201(f) (United States interoperability exemption)
- Fair Use / Fair Dealing doctrines where applicable

The implementation follows clean-room style engineering practices for independent behavior verification and format compatibility.

Users are responsible for ensuring their use complies with applicable law in their jurisdiction.

**Disclaimer:** This section is general information only and does not constitute legal advice. For jurisdiction-specific guidance, consult qualified legal counsel.
