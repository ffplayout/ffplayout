The configuration file **ffplayout.yml** have this sections:

---

```YAML
general:
    stop_on_error: True
    stop_threshold: 11
```
sometimes it can happen, that a file is corrupt but still playable,
this can produce an streaming error over all following files.
The only way in this case is, to stop ffplayout and start it again
here we only say it can stop, the starting process is in your hand
best way is a **systemd serivce** on linux.
`stop_threshold:` stop ffplayout, if it is async in time above this value.

---

```YAML
mail:
    subject: "Playout Error"
    smpt_server: "mail.example.org"
    smpt_port: 587
    sender_addr: "ffplayout@example.org"
    sender_pass: "12345"
    recipient:
    mail_level: "ERROR"
```
Send error messages to email address, like:
- missing playlist
- unvalid json format
- missing clip path
leave recipient blank, if you don't need this.
`mail_level` can be: **WARNING, ERROR**

---

```YAML
logging:
    log_to_file: True
    backup_count: 7
    log_path: "/var/log/ffplayout/"
    log_level: "DEBUG"
    ffmpeg_level: "ERROR"
```

Logging to file, if `log_to_file = False` > log to console.
`backup_count` says how long log files will be saved in days.
Path to **/var/log/** only if you run this program as *deamon*.
`log_level` can be: **DEBUG, INFO, WARNING, ERROR**
`ffmpeg_level` can be: **INFO, WARNING, ERROR**

---

```YAML
processing:
    width: 1024
    height: 576
    aspect: 1.778
    fps: 25
    add_logo: True
    logo: "docs/logo.png"
    logo_scale: "100:-1"
    logo_opacity: 0.7
    logo_filter: "overlay=W-w-12:12"
    add_loudnorm: False
    loud_I: -18
    loud_TP: -1.5
    loud_LRA: 11
    output_count: 1
```

ffmpeg pre-compression settings, all clips get prepared in that way,
so the input for the final compression is unique.
- `aspect` mus be a float number.
- with `logo_scale = 100:-1` logo can be scaled
- with `logo_opacity` logo can make transparent
- with `logo_filter = overlay=W-w-12:12` you can modify the logo position
- with use_loudnorm you can activate single pass EBU R128 loudness normalization
- loud_* can adjust the loudnorm filter
- `output_count` sets the outputs for the filtering, > 1 gives the option to use the same filters for multiple outputs. This outputs can be taken in 'ffmpeg_param', names will be vout2, vout3;
aout2, aout2 etc.

**INFO:** output is progressive!

---

```YAML
playlist:
    playlist_mode: True
    path: "/playlists"
    day_start: "5:59:25"
    length: "24:00:00"
```
Playlist settings -
set `playlist_mode` to **False** if you want to play clips from the `storage:` section
put only the root path here, for example: **"/playlists"**.
Subfolders is read by the script and needs this structur:
- **"/playlists/2018/01"** (/playlists/year/month)

`day_start` means at which time the playlist should start. Leave `day_start` blank when playlist should always start at the begin.
`length` represent the target length from playlist, when is blank real length will not consider.

---

```YAML
storage:
    path: "/mediaStorage"
    filler_clip: "/mediaStorage/filler/filler.mp4"
    extensions:
        - ".mp4"
        - ".mkv"
    shuffle: True
```
Play ordered or ramdomly files from path, `filler_clip` is for fill the end
to reach 24 hours, it will loop when is necessary. `extensions:` search only files
with this extension, add as many as you want. Set `shuffle` to **True** to pick files randomly.

---

```YAML
text:
    add_text: True
    over_pre: False
    bind_address: "tcp://127.0.0.1:5555"
    fontfile: "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf"
    text_from_filename: False
    style: "x=(w-tw)/2:y=(h-line_h)*0.9:fontsize=24:fontcolor=#ffffff:box=1:boxcolor=#000000:boxborderw=4"
    regex: "^(.*)_"
```
Overlay text in combination with [messenger](https://github.com/ffplayout/messenger) or the web [frontend](https://github.com/ffplayout/ffplayout-frontend).
On windows `fontfile` path need to be like this: **C\:/WINDOWS/fonts/DejaVuSans.ttf**.
In a standard environment the filter drawtext node is: **Parsed_drawtext_2**.
`over_pre` if True text will be overlay in pre processing. Continue same text
over multiple files is in that mode not possible.
`text_from_filename` activate the extraction from text of a filename. With `style` you can define the drawtext parameters like position, color, etc. Post Text over API will override this.
With `regex` you can format file names, to get a title from it.

---

```YAML
out:
    mode: 'stream'
    service_name: "Live Stream"
    service_provider: "example.org"
    ffmpeg_param: >-
        -c:v libx264
        -crf 23
        -x264-params keyint=50:min-keyint=25:scenecut=-1
        -maxrate 1300k
        -bufsize 2600k
        -preset medium
        -profile:v Main
        -level 3.1
        -c:a aac
        -ar 44100
        -b:a 128k
    stream_output: >-
        -flags +global_header
        -f flv rtmp://localhost/live/stream
    hls_output: >-
        -flags +cgop
        -f hls
        -hls_time 6
        -hls_list_size 600
        -hls_flags append_list+delete_segments+omit_endlist+program_date_time
        -hls_segment_filename /var/www/srs/live/stream-%09d.ts /var/www/srs/live/stream.m3u8
```

The final ffmpeg post compression, Set the settings to your needs!
`mode` has the standard options **desktop**, **hls**, **stream**. Self made outputs
can be define, by adding script in output folder with an 'output' function inside.
