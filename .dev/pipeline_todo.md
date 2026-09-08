# Pipeline implementation backlog

This backlog covers the configuration storage prepared by migration 4
(`00004_pipeline_options.sql`). The migration intentionally only creates safe
defaults; none of the options below are active until its work package is
implemented and tested.

Each package is designed to be completed independently by a future agent. Do
not expose a field in the API or frontend before its engine path validates and
uses it.

## Shared rules

- Preserve the current defaults and behaviour when every new option is empty.
- Keep FFmpeg option dictionaries scoped to their layer: protocol, demuxer,
  encoder, muxer, or device. Do not pass one dictionary to another layer.
- Reject unknown, empty, incompatible, or security-sensitive options before
  saving configuration. Never silently ignore options left by FFmpeg.
- Keep enforced operational settings, such as network timeouts and HLS segment
  management, under ffplayout's control.
- Add database, API, engine, and UI tests as part of the same package.
- Migration 4 is unstable only until the next release that contains it. After
  that release, add new migrations instead of editing it.

## 1. Source protocol and demuxer options

**Stored fields:** `config_source.protocol_options`,
`config_source.demuxer_options`

Implement options used while opening playlist media sources. These options are
separate from `config_live_input`, which configures takeover sources.

- [ ] Add `Source` to the application configuration, database model, and
  configuration API.
- [ ] Apply `protocol_options` only when opening network URLs.
- [ ] Apply `demuxer_options` only to FFmpeg input/demuxer contexts.
- [ ] Preserve and enforce the existing `rw_timeout`; user input must not be
  able to disable it.
- [ ] Validate option names and values against the selected FFmpeg protocol or
  demuxer without opening an untrusted network connection where possible.
- [ ] Add a small key/value UI editor with i18n and explanatory safety text.

**Acceptance criteria:** empty configuration has no effect; valid options are
consumed by FFmpeg; invalid options fail before saving; network and local-file
sources receive only options intended for them.

## 2. Generic live-input backends and takeover

**Stored table:** `config_live_input`

Implement selected, long-running live sources. The existing RTMP listener is
already represented by its migrated `rtmp`/`connection` entry. A source being
reachable must not by itself be treated as either a request to take over
playout or as a reliable end of a programme.

- [ ] Add configuration models, CRUD API, validation, capability reporting,
  and a UI for prioritised live-input entries.
- [ ] Define initial backend identifiers: `rtmp`, `srt_listener`, `ndi`, and
  `decklink`; treat `identifier` as respectively a URL/listen endpoint,
  discovered NDI source name, or a stable hardware-device name.
- [ ] Keep backend-specific settings in `options`; validate them per backend
  and never pass them across protocol, device, or FFmpeg layers.
- [ ] Implement all stored takeover modes: `manual`, `external_trigger`,
  `signal_presence`, `connection`, and `duration`.
- [ ] Arbitrate simultaneous takeover requests by descending `priority` and
  stable `id`; an active takeover is not pre-empted automatically.
- [ ] Apply `signal_loss_grace_seconds` before returning to the playlist, so
  temporary NDI/network or SDI signal loss cannot flap playout.
- [ ] Enforce `max_duration_seconds` as a hard safety stop; `0` means no limit.
- [ ] Model state separately from configuration: availability, takeover active,
  last signal, and actionable failure reason must be observable but must not be
  persisted as static configuration.
- [ ] Preserve the current RTMP listener behaviour through its migrated
  `rtmp`/`connection` entry; its API/UI fields may be renamed only alongside a
  backwards-compatible API transition.
- [ ] Keep protocol-based live playlist-source detection separate from
  takeover state; HTTP(S) remains classified as ordinary remote media.

**Acceptance criteria:** an idle NDI or DeckLink source never interrupts the
playlist; a configured trigger starts takeover; loss/end returns to the
playlist only after its grace period; a stuck source is stopped at its maximum
duration; ordinary RTMP ingest remains backwards compatible.

## 3. Output protocol options

**Stored field:** `config_output.protocol_options`

Implement transport-level output settings separately from existing
`muxer_options`.

- [ ] Define supported options per output transport: RTMP, SRT, UDP, and
  custom network outputs.
- [ ] Merge validated user settings with enforced output timeout settings when
  the FFmpeg output context is opened.
- [ ] Explicitly decide how secrets such as SRT passphrases are stored and
  redacted from logs and API responses before exposing them in the UI.
- [ ] Reject irrelevant options for file, HLS, and desktop outputs.

**Acceptance criteria:** examples such as SRT latency and UDP packet size work;
unsupported or unsafe settings are rejected without affecting a running output.

## 4. Audio encoder options

**Stored fields:** `config_output.audio_options`,
`config_recording.audio_options`

Add the audio counterpart to the existing validated video encoder options.

- [ ] Extend output and re-encode recording configuration models and update
  paths.
- [ ] Add FFmpeg encoder-option discovery/validation comparable to
  `video_options`.
- [ ] Apply options to the audio encoder context and fail if FFmpeg leaves an
  option unused.
- [ ] Provide codec-specific defaults and an advanced UI editor.

**Acceptance criteria:** AAC/Opus or other supported codec options survive a
save/reload cycle, affect the encoder, and invalid values are rejected before
the output is restarted.

## 5. Program audio layout

**Stored field:** `config_audio.program_layout`

Generalize the internal program-audio bus beyond the current fixed stereo
pipeline.

- [ ] Define supported layout names and validate them using FFmpeg channel
  layouts.
- [ ] Replace fixed `ChannelLayout::STEREO` assumptions in decoding, resampling,
  mixing, silence generation, loudness analysis, encoding, and tests.
- [ ] Decide and document deterministic upmix/downmix behaviour for mismatched
  source layouts.
- [ ] Keep desktop output stereo until it has an explicit multichannel device
  implementation; reject unsupported layouts there rather than dropping
  channels silently.

**Acceptance criteria:** mono, stereo, and at least 5.1 program audio remain
sample-synchronised with video and pass through an encoded output without
channel loss or reordering.

## 6. Multiple output audio tracks

**Stored table:** `config_output_audio`

Implement independent output tracks after the program-audio bus supports the
necessary layouts.

- [ ] Define valid `source` values and `source_index` semantics, beginning
  with the program bus.
- [ ] Implement track ordering, title, BCP-47 language tag, default-track
  disposition, codec, bitrate, encoder options, and channel mapping.
- [ ] Create one encoder and muxed stream per configured track.
- [ ] Support stream/HLS metadata and verify client-visible language/default
  metadata in generated playlists or containers.
- [ ] Decide separately how recordings inherit or override output tracks.

**Acceptance criteria:** two differently configured tracks are encoded,
interleaved, labelled, and selectable by a consuming player; duplicate track
positions remain impossible at database and API level.

## 7. Hardware device output foundation

**Stored fields:** `config_output.device_backend`,
`device_identifier`, `device_options`

Introduce a hardware output abstraction without conflating it with the current
window-based `desktop` output.

- [ ] Add a `device` output mode and a backend interface selected by
  `device_backend`.
- [ ] Keep device discovery separate from persistent configuration; store a
  stable identifier only after a user selects a discovered device.
- [ ] Validate backend-specific `device_options` and expose only options
  supported by the selected device.
- [ ] Gate device backends behind explicit Cargo features and report a clear
  configuration error when a configured backend is unavailable in the build.

**Acceptance criteria:** an unavailable device backend fails clearly and does
not affect HLS, stream, or desktop outputs; device configuration round-trips
without exposing unsupported settings.

## 8. DeckLink output backend

**Depends on:** packages 5 and 7.

- [ ] Choose and document the integration path: FFmpeg `libavdevice` with
  DeckLink support, or the Blackmagic Desktop Video SDK.
- [ ] Add build detection and feature gating for the required SDK/libraries.
- [ ] Implement device discovery, video mode selection, connection selection,
  pixel format, embedded-audio channel count, and audio-pair mapping.
- [ ] Define format negotiation and errors for unsupported FPS, resolution,
  pixel format, or audio layout.
- [ ] Add hardware-independent unit tests plus opt-in hardware integration
  tests that are skipped unless a DeckLink device is explicitly configured.

**Acceptance criteria:** a selected DeckLink device receives video and the
configured embedded audio layout with stable clocking; unsupported hardware or
formats fail before playout starts.

## 9. NDI live-input backend

**Depends on:** package 2.

- [ ] Build FFmpeg with the NDI SDK under an explicit Cargo/build feature and
  expose whether that capability is available.
- [ ] Implement discovery and selection by the stable NDI source name.
- [ ] Map receiver availability and reconnect events to observable runtime
  state, not directly to playout takeover.
- [ ] Support `manual` and `external_trigger` first; add `signal_presence`
  only after its behaviour is verified against real sources.
- [ ] Add opt-in integration tests using a local NDI sender, while keeping
  normal CI hardware- and SDK-independent.

**Acceptance criteria:** a disappeared NDI sender reconnects without a process
restart; it never starts or stops playout unless its configured takeover policy
requires that action.

## 10. DeckLink live-input backend

**Depends on:** packages 2 and 5.

- [ ] Choose and document FFmpeg `libavdevice` or direct DeckLink SDK capture;
  initially prefer the FFmpeg path.
- [ ] Implement discovery, input-mode/connection selection, embedded-audio
  channel count, and a clear unavailable-device error.
- [ ] Use `manual`/`external_trigger` as initial defaults. Implement
  `signal_presence` only with signal-loss hysteresis and tested format-change
  handling.
- [ ] Constrain the initial implementation to supported program layouts; do
  not silently discard embedded audio channels.
- [ ] Add opt-in hardware integration tests that require an explicitly named
  DeckLink device.

**Acceptance criteria:** an SDI source can be taken on air and returned to the
playlist predictably; signal loss cannot cause rapid takeover flapping.

## 11. End-to-end hardware frame pipeline

**Depends on:** packages 5, 7, and the chosen decoder/filter backend.

The current path uses CPU decoding/compositing and uploads only immediately
before a hardware encoder. Treat a full hardware path as a separate project.

- [ ] Introduce a hardware-device and frame-context manager shared by decoder,
  processing, encoder, and device output.
- [ ] Define supported paths explicitly, for example VAAPI decode → processing
  → VAAPI encode, rather than claiming generic zero-copy support.
- [ ] Keep CPU fallback and deterministic format conversion for every hardware
  boundary.
- [ ] Benchmark transfer points and add capability reporting to the API/UI.

**Acceptance criteria:** each advertised path is feature-gated, observable in
logs/capabilities, has a CPU fallback, and is tested without regressing the
existing CPU pipeline.
