# Graphics control (experimental)

The **Graphics** page groups manual graphics, existing text controls and the
program logo in three tabs. The former `/message` link redirects to the Text tab.
Existing text presets, logo database fields and configuration APIs remain valid.
Migration 8 includes per-channel graphics settings and revision storage. The known
earlier version of migration 8 upgrades automatically after the normal database
backup; unrelated checksum changes are still rejected.

## Manual graphics

Select a local PNG, GIF or video from channel storage. Set a layer ID, program-pixel
position, optional source size, opacity, z-index and fade/source repetition settings.
The placement preview supports dragging and arrow keys (2 pixels, Shift: 10 pixels).
Its corner handle resizes proportionally using the source aspect ratio and enables
source-size override. The handle also supports arrow keys (2 program pixels of
width, Shift: 10). Sizes stay even and within 2..8192 pixels for YUVA composition.
Fit into program canvas makes oversized graphics fully visible and their handles
reachable. Position and size controls stay in sync. Save/Take or a fresh clip start
applies a changed source size; Apply layout and fades does not reload source assets.
It is a browser placement aid: it does not simulate the video background, exact
compositing colors, fades or source timing. Browser-incompatible codecs retain an
editable position rectangle when metadata is available.

**Take** prepares a source off the video thread and replaces the same manual layer
only after preparation succeeds. Untimed manual graphics hold until stopped;
finite duration and repetition use the same timing rules as playlist graphics.
**Stop** uses that take's fade-out and changes the button to **Start**.
**Start** restarts the prepared source, duration and fade-in from the beginning,
retaining its current saved layout. It also restores visibility. Already prepared
matching sources are reused; after a restart or saved source change they are
prepared on demand. **Clear** removes the layer immediately. **Show/hide**
uses its configured fade-in/fade-out, preserving the source clock and finite
lifetime. Reversing a visibility fade continues from its current opacity. Status
reports requested visibility separately from frames still active during fade-out.
Layout updates also preserve the source clock. At most eight manual layers are kept.

Manual layer definitions, names and requested running/visibility states are saved
per channel. **Save settings** is available while the channel is stopped and does
not change current playback; **Take** saves and starts the edited definition.
Each layer has its own DaisyUI collapse editor. Polling does not overwrite unsaved
edits. **Apply layout and fades** saves/applies position, opacity, z-index and fade times.
New fade times are used by subsequent Show/Hide and Stop commands, including
after Save settings. This does not reload the source, restart its animation,
retime scheduled fade cycles or alter a fade already in progress. Source and
finite-duration settings still take effect on the next Take/Start.

Channel stops release runtime resources without deleting settings. On restart,
started visible layers prepare in the background and begin again with fade-in.
Started hidden layers remain hidden and prepare lazily when shown. Stopped layers
load settings only. Fade progress, repeat phase and source position are not stored.
Missing/broken sources report a per-layer error without failing program playback
or other layers. Clear/Clear all also delete the stored definitions.
They do not change playlist graphics, the program logo or text presets. Within the
manual group, `(z_index, layer ID)` determines order. The program order is logo,
playlist graphics, manual graphics, configured text, runtime text. Legacy groups
remain separate z-index domains. Desktop, recordings and encoders use the shared
program frame; HLS still composites the largest rendition before downscaling.

The UI shows preparation progress while Take is pending and reports errors without
removing previously working graphics. Status is refreshed once per second while
this tab is mounted. A revision conflict requires refreshing before retrying; the
UI refreshes status after every command. Navigating away does not clear graphics.

## Program logo

All existing logo settings have moved from General Settings to **Graphics → Program
logo**. A link remains in General Settings. Animated sources continue looping.
Saving updates only the five logo fields, even if other settings have unsaved edits.
Editing requires channel/global administrator rights; authorized channel users can
view the settings. Logo changes apply during playback and live ingest without a
channel restart. New files and sizes are prepared in the background; the active
logo continues until the replacement is ready. Failed replacements retain the
working source. Position and opacity changes reuse decoded pixels and preserve
the animation clock, including hide/show via zero opacity. Destination-relative
placement and largest-rendition HLS composition remain unchanged.

One lazy, tracked logo-update worker handles preparation handoff and decoder
retirement. It parks while idle, keeps only the latest request and one pending
replacement, and uses the channel preparation budget/lifecycle. Frame processing
only polls ready results; it performs no file I/O, decoder joins or blocking
control-lock acquisition. Channel shutdown cancels/joins the worker through the
existing graphics preparation service. Other pending configuration changes can
still require a restart; editing the logo itself does not.

## HTTP API

- `GET /api/graphics/{channel}` returns `revision`, `running`, the active program `canvas` and manual layer status. Each layer reports requested `running` state separately
  from `active` frames (including outgoing fades) and `visible` intent.
  Take and Start set `running` to true immediately; only Stop sets it to false.
  Transparent, hidden or naturally completed graphics retain that requested state.
- `POST /api/graphics/{channel}` accepts an action with the last observed `revision`.
  Supported actions: `save`, `take`, `update`, `start`, `stop`, `visibility`, `clear`, `clear_all`.
  Save/Take accept `{action, revision, layer, name, settings}`; `settings` is a
  playlist Take definition with time fixed at zero. Legacy flattened Take bodies
  remain accepted. Status includes each layer's full `settings`, `name` and `error`.
- `PUT /api/playout/config/{channel}?scope=logo` saves only the program-logo fields.

All routes enforce authentication and channel access. Manual operations permit the
same roles as existing text controls. Logo configuration retains administrator-only
writes. Commands with obsolete revisions fail with 409. Preparation is limited to
two concurrent requests per channel, shares the channel's graphics resource budget,
and cancels restoration/preparation at channel shutdown. State-changing commands
are serialized per channel; DB snapshots and revision updates are transactional.
The video thread never waits for HTTP requests, file loading or a control mutex;
a busy control mutex retains the last frame snapshot. Immutable queued snapshots
remain valid after a control change. A bounded metadata pool reuses frame storage.

Reusable graphic templates, SVG, richer transitions and GPU/Servo remain
separate features in `.dev/graphics_todo.md`.

The position canvas uses a checkerboard to show transparency. While dragging, a graphic snaps to the horizontal and vertical program center within six preview pixels. Alignment guides appear while snapped. Arrow keys remain available for unsnapped fine adjustments.

When either displayed graphic dimension is below 48 browser pixels, the canvas keeps only the bottom-right resize handle at the interaction-frame corner and expands the dashed drag area to at least 48 pixels per axis (bounded by the canvas). The image retains its actual size and position; numeric size fields remain available. The threshold follows browser preview size, including responsive resizing.
