### Stream Copy

ffplayout supports a stream copy mode since v0.20.0. A separate copy mode for video and audio is possible. This mode uses less CPU and RAM, but has some drawbacks:

- All files must have exactly the same resolution, color depth, audio channels and kHz.
- All files must use the same codecs and settings.
- The video and audio lines of a file must be the same length.
- The codecs and A/V settings must be supported by mpegts and the output destination.
- If the output mode is HLS, the time delta will increase over time, so the error threshold should be high enough to catch this.

**This mode is experimental and will not have the same stability as the stream mode.**
