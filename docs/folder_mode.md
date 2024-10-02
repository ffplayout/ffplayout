### Folder Mode

ffplayout can play files from a folder; no playlists are required for this mode. This folder is monitored for changes, and when new files are added or deleted, they are registered and updated accordingly.

You just need to set `mode: folder` in the config under `processing:`, and under `storage:`, you have to specify the correct folder and the file extensions you want to scan for.

Additionally, there is a **shuffle** mode. If this is activated, the files will be played randomly.

If shuffle mode is off, the clips will be played in sorted order.
