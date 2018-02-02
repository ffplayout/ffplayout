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
<smil>
	<head>
		<meta name="author" content="Author"/>
		<meta name="title" content="Title"/>
		<meta name="copyright" content="(c)2018 company"/>
	</head>
	<body>
		<video src="/path/clip_01.mkv" clipBegin="21600s" dur="18.000000s" in="0.00" out="18.000000s"/>
		<video src="/path/clip_02.mkv" clipBegin="21618s" dur="18.111000s" in="0.00" out="18.111000s"/>
		<video src="/path/clip_03.mkv" clipBegin="21636.1s" dur="247.896000s" in="0.00" out="247.896000s"/>
		<video src="/path/clip_04.mkv" clipBegin="21884s" dur="483.114000s" in="0.00" out="483.114000s"/>
		<video src="/path/clip_05.mkv" clipBegin="22367.1s" dur="20.108000s" in="0.00" out="20.108000s"/>
		<video src="/path/clip  &amp; specials.mkv" clipBegin="22387.2s" dur="203.290000s" in="0.00" out="203.290000s"/>
		<video src="/path/clip_06.mkv" clipBegin="22590.5s" dur="335.087000s" in="300.00" out="335.087000s"/>
	</body>
</smil>
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
