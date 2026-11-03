# Graphics control (experimental)

The **Graphics** page groups manual graphics, existing text controls and the
program logo in three tabs. The former `/message` link redirects to the Text tab.
Existing text presets, logo database fields and configuration APIs remain valid.
No database migration is required.

## Manual graphics

Select a local PNG, GIF or video from channel storage. Set a layer ID, program-pixel
position, optional source size, opacity, z-index and fade/source repetition settings.
The placement preview supports dragging and arrow keys (2 pixels, Shift: 10 pixels).
It is a browser placement aid: it does not simulate the video background, exact
compositing colors, fades or source timing. Browser-incompatible codecs retain an
editable position rectangle when metadata is available.

**Take** prepares a source off the video thread and replaces the same manual layer
only after preparation succeeds. Untimed manual graphics hold until stopped;
finite duration and repetition use the same timing rules as playlist graphics.
**Stop** uses that take's fade-out; **Clear** removes it immediately. Visibility
and layout updates preserve the source clock. At most eight manual layers are kept.

Manual graphics persist across file changes and live ingest, until cleared or the
channel stops/restarts. They are transient and are not restored after a restart.
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

- `GET /api/graphics/{channel}` returns `revision`, `running`, the active program `canvas` and manual layer status.
- `POST /api/graphics/{channel}` accepts an action with the last observed `revision`.
  Supported actions: `take`, `update`, `stop`, `visibility`, `clear`, `clear_all`.
  A take uses the playlist take fields, with time fixed at zero for this occurrence.
- `PUT /api/playout/config/{channel}?scope=logo` saves only the program-logo fields.

All routes enforce authentication and channel access. Manual operations permit the
same roles as existing text controls. Logo configuration retains administrator-only
writes. Commands with obsolete revisions fail with 409. Preparation is limited to
two concurrent requests per channel, shares the channel's graphics resource budget,
and rejects completion after clear/restart or any intervening accepted command.
The video thread never waits for HTTP requests, file loading or a control mutex;
a busy control mutex retains the last frame snapshot. Immutable queued snapshots
remain valid after a control change. A bounded metadata pool reuses frame storage.

Persistent graphic presets/templates, SVG, richer transitions and GPU/Servo remain
separate features in `.dev/graphics_todo.md`.
