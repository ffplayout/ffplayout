# Pipeline implementation backlog

This backlog covers the configuration storage prepared by migration 4 and 5
(`00004_pipeline_options.sql` and `00005_output_metadata_and_live_listeners.sql`). The migrations intentionally create safe
defaults; options below remain inactive until their work package is
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
- Migration 5 is unstable only until the next release that contains it. After
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

## 2. RTMP/SRT live listeners and takeover

**Stored table:** `config_live_input`

RTMP and SRT `connection` listeners already use this table and run concurrently.
The engine selects the highest-priority ready listener (stable ID as
tie-breaker) without pre-empting an active takeover. Protocol and demuxer
options are separate; `format=live_flv` is an explicit RTMP demuxer override,
not a default. Only the RTMP/SRT connection-listener work belongs here.

- [x] Add configuration models, save/load API, validation, and a UI for
  prioritised RTMP/SRT listeners. The supported FFmpeg input protocol is
  checked when an enabled listener is saved.
- [x] Use `rtmp` and `srt` backend identifiers with listen URLs in
  `identifier`; support multiple concurrent listeners on distinct ports.
- [x] Validate protocol `options` and separate `demuxer_options` per backend;
  pass each map only to its corresponding FFmpeg input layer.
- [x] Implement `connection` takeover on the first decoded video frame.
- [x] Arbitrate ready listeners by descending `priority` (0–100) and stable
  `id`; an active takeover is not pre-empted automatically.
- [x] Migrate the former RTMP address and enabled state into its
  `rtmp`/`connection` entry; the legacy API fields are not retained.
- [ ] Expose per-listener runtime state and actionable failure reasons
  separately from persisted configuration (for example listening, connected,
  on air, and last failure).
- ~~Expose and enforce `max_duration_seconds` as a hard cutoff for RTMP/SRT connection listeners.~~
  Not planned: a fixed limit could interrupt a legitimate long broadcast.

**Acceptance criteria:** RTMP and SRT listeners can wait concurrently without
blocking one another; priority selects among ready listeners but never
pre-empts one already on air; ending the active input returns to the playlist
or selects a still-ready listener.

## 2a. NDI/DeckLink live-input foundation

**Stored table:** `config_live_input`; **depends on:** package 2.

Prepare the shared takeover contract for persistent discoverable sources before
implementing the NDI and DeckLink backends in packages 9 and 10. A reachable
device or present signal alone must not automatically interrupt the playlist.

- [ ] Define `ndi` and `decklink` identifiers as a discovered NDI source name
  or stable hardware-device name, not a network listen URL.
- [ ] Add backend capability reporting and discovery; reject a configured
  backend that is unavailable in the current build.
- [ ] Validate backend-specific `options` and keep device settings separate
  from protocol and demuxer option dictionaries.
- [ ] Define and implement the stored `manual`, `external_trigger`,
  `signal_presence`, and `duration` takeover modes where supported. Preserve
  the existing RTMP/SRT `connection` behaviour.
- [ ] Apply `signal_loss_grace_seconds` before ending such a takeover, so
  temporary NDI or SDI signal loss cannot flap playout.
- [ ] Reuse the shared priority/no-preemption rules and expose per-device
  availability, last signal, active takeover, and failure reason as runtime
  state rather than static configuration.

**Acceptance criteria:** an idle NDI or DeckLink source never interrupts the
playlist; only its configured takeover policy starts playback; signal loss
returns to the playlist after the configured grace period; unsupported
hardware or modes fail clearly.

## 3. Output protocol options

**Stored field:** `config_output.protocol_options`

Implement transport-level output settings separately from existing
`muxer_options`.

- [x] Define supported options per output transport: RTMP, SRT, UDP, and
  custom network outputs.
- [x] Merge validated user settings with enforced output timeout settings when
  the FFmpeg output context is opened.
- [x] Explicitly decide how secrets such as SRT passphrases are stored and
  redacted from logs and API responses before exposing them in the UI.
- [x] Reject irrelevant options for file, HLS, and desktop outputs.

SRT passphrases and other protocol values intentionally use the same storage
and authenticated configuration API representation as the existing advanced
option maps: they are stored and returned unchanged. Output URLs and option
dictionaries must not be written to logs. A future masked-secret API can be
added as a separate hardening measure without changing the FFmpeg pipeline.

**Acceptance criteria:** examples such as SRT latency and UDP packet size work;
unsupported or unsafe settings are rejected without affecting a running output.

## 3a. Output container metadata

**Stored field:** `config_output.metadata_options` (JSON map in a new
migration; do not add dedicated `service_name` or `service_provider` columns).

Output metadata belongs to the FFmpeg output context, not to its muxer or
protocol option dictionaries. MPEG-TS reads `service_name` and
`service_provider` from this metadata to populate the DVB service description.

- [x] Add a generic output metadata map to the database, API, engine config,
  and advanced UI with translations.
- [x] Validate metadata keys, values, and size before saving. Container-specific
  tag support remains the responsibility of the selected FFmpeg muxer.
- [x] Set output-context metadata before writing the muxer header. Do not pass
  metadata through `muxer_options` or `protocol_options`.
- [x] Verify the resulting MPEG-TS service metadata with a container-level
  regression test; preserve FFmpeg defaults when the map is empty.

**Acceptance criteria:** an SRT, UDP, or custom MPEG-TS stream can advertise a
configured DVB service name and provider; other formats can receive arbitrary
global tags where their muxer supports them. Invalid metadata is rejected and
empty configuration keeps FFmpeg defaults.

## 4. Audio encoder options

**Stored fields:** `config_output.audio_options`,
`config_recording.audio_options`

Add the audio counterpart to the existing validated video encoder options.

- [x] Extend output and re-encode recording configuration models and update
  paths.
- [x] Add FFmpeg-native encoder-option validation comparable to
  `video_options`; users enter option names explicitly in the advanced editor.
- [x] Apply options to the audio encoder context and fail if FFmpeg leaves an
  option unused.
- [x] Provide an advanced UI editor while leaving the option map empty by
  default so FFmpeg retains control of codec defaults.

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

**Depends on:** packages 2 and 2a.

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

**Depends on:** packages 2, 2a, and 5.

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
