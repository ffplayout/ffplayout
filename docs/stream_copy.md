### Stream Copy

ffplayout has supported a stream copy mode. A separate copy mode for video and audio is possible. This mode uses less CPU and RAM but has some drawbacks:

- All files must have exactly the same resolution, framerate, color depth, audio channels, and kHz.
- All files must use the same codecs and settings.
- The video and audio lines of a file must be the same length.
- The codecs and A/V settings must be supported by MPEG-TS and the output destination.

**This mode is experimental and will not have the same stability as the stream mode.**
