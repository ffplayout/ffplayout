**ffplayout**
================


This is a streaming solution based on python and ffmpeg.

The goal is to play for every day an xml playlist, while the current playlist is still editable.


Features
-----

- have all values in a separate config file
- try to be as simple as possible
- dynamic playlist
- replace missing playlist or clip with a blank clip
- send emails with error message
- overlay a logo
- trim and fade the last clip, to get full 24 hours, if the duration is less then 6 seconds add a blank clip
- set custom day start, so you can have playlist for example: from 6am to 6am, instate of 0am to 12pm
- normal system requirements and no special tools
    - we only need **ffmpeg**, **ffprobe** and a buffer tool like **mbuffer** or **pv**
    - no GPU power is needed
    - ram and cpu depends on video resolution, minimum 4 threads and 3GB ram for 720p are recommend
- python version 3.5 and up

XML Playlist Example
-----

```xml
<playlist>
    <head>
        <meta name="author" content="example"/>
        <meta name="title" content="Live Stream"/>
        <meta name="copyright" content="(c)2018 example.org"/>
        <meta name="date" content="2018-02-03"/>
    </head>
    <body>
        <video src="/path/clip_01.mkv" begin="21600" dur="18.000000" in="0.00" out="18.000000"/>
        <video src="/path/clip_02.mkv" begin="21618" dur="18.111000" in="0.00" out="18.111000"/>
        <video src="/path/clip_03.mkv" begin="21636.1" dur="247.896000" in="0.00" out="247.896000"/>
        <video src="/path/clip_04.mkv" begin="21884" dur="483.114000" in="0.00" out="483.114000"/>
        <video src="/path/clip_05.mkv" begin="22367.1" dur="20.108000" in="0.00" out="20.108000"/>
        <video src="/path/clip  &amp; specials.mkv" begin="22387.2" dur="203.290000" in="0.00" out="203.290000"/>
        <video src="/path/clip_06.mkv" begin="22590.5" dur="335.087000" in="300.00" out="335.087000"/>
    </body>
</playlist>
```

Installation
-----
- install ffmpeg, ffprobe and mbuffer
- copy, or symlink, ffplayout.py to **/usr/local/bin/**
- copy, or symlink, ffplayout.conf to **/etc/ffplayout/**
- copy ffplayout.service to **/etc/systemd/system/**
- change user in service file
- create playlists folder, in that format: **/playlists/year/month**
- set variables in config file to your needs
- use **get_playlist_from_subfolders.sh /path/to/*.mp4s** as a starting point for your playlists (path in script needs to change)
- activate service and start it: **sudo systemctl enable ffplayout && sudo systemctl start ffplayout**




TODO
-----
- better xml validation
- time sync check and correction
- check empty playlist
- check when clip or playlist got lost while playling (?)
- add support for clip out point, in playlist
