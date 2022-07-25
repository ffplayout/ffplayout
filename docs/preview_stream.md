### Preview Stream

The ffplayout engine has no special preview config parameters, but you can add your settings to the **output_param**, like:

```YAML
    -s 512x288
    -c:v libx264
    -crf 24
    -x264-params keyint=50:min-keyint=25:scenecut=-1
    -maxrate 800k
    -bufsize 1600k
    -preset ultrafast
    -tune zerolatency
    -profile:v Main
    -level 3.1
    -c:a aac
    -ar 44100
    -b:a 128k
    -flags +global_header
    -f flv rtmp://preview.local/live/stream

    ...
```

In this documentation we suspect, that you are using [ffplayout-frontend](https://github.com/ffplayout/ffplayout-frontend) and that you using [SRS](https://github.com/ossrs/srs) at least for the preview stream. In the past we used HLS for the preview, but now it is possible to also use [HTTP-FLV](https://github.com/ossrs/srs/wiki/v4_EN_DeliveryHttpStream) for less latency.

To get this working we have to follow some steps. ffplayout engine needs a direction where it can stream on and SRS need a virtual host for the rtmp input. Because both runs on the same machine, we use for that a redirection in the **/etc/hosts** file:

```
...
127.0.0.1   preview.local
```

Add this line to **/etc/hosts** ans save it.

#### Next step is to compile and install SRS:

```BASH
# install some tool for compiling
apt install curl wget net-tools git build-essential autoconf automake libtool pkg-config gperf libssl-dev

cd /opt/

# get SRS
git clone https://github.com/ossrs/srs.git

cd srs/trunk

# get correct branch
git checkout 4.0release

./configure --ffmpeg-fit=off

make -j4

# install SRS to /usr/local/srs
make install

```

Now we need a systemd service, to startup SRS automatically. Create the file:

**/etc/systemd/system/srs.service**

with this content:

```INI
Description=SRS
Documentation=https://github.com/ossrs/srs/wiki
After=network.target

[Service]
Type=forking
ExecStartPre=/usr/local/srs/objs/srs -t -c /etc/srs/srs.conf
ExecStart=/usr/local/srs/objs/srs -c /etc/srs/srs.conf
ExecStop=/bin/kill -TERM \$MAINPID
ExecReload=/bin/kill -1 \$MAINPID
Restart=always
RestartSec=3

[Install]
WantedBy=multi-user.target
```

Then create the config for SRS under **/etc/srs/srs.conf** with this content:

```NGINX
listen              1935;
max_connections     20;
daemon              on;
pid                 /usr/local/srs/objs/srs.pid;
srs_log_tank        console; # file;
srs_log_file        /var/log/srs.log;
ff_log_dir          /tmp;
srs_log_level       error;

http_server {
    enabled         on;
    listen          127.0.0.1:8080;
    dir             ./objs/nginx/html;
}

stats {
    network         0;
    disk            sda vda xvda xvdb;
}

vhost preview.local {
    http_remux {
        enabled     on;
        mount       [vhost]/[app]/[stream].flv;
    }
}
```

Now you can enable and start SRS with: `systemctl enable --now srs` and check if it is running: `systemctl status srs`

#### Configure Nginx

We assume that you have already installed nginx and you are using it already for the frontend. So open the frontend config **/etc/nginx/sites-enabled/ffplayout.conf** and add a new location to it:

```NGINX
location /preview/stream.flv {
    proxy_pass http://preview.local:8080/live/stream.flv;
}
```

Full config looks like:

```NGINX
server {
    listen 80;

    server_name ffplayout.example.org;

    gzip on;
    gzip_types text/plain application/xml text/css application/javascript;
    gzip_min_length 1000;

    charset utf-8;

    client_max_body_size 7000M; # should be desirable value

    add_header X-Frame-Options SAMEORIGIN;
    add_header X-Content-Type-Options nosniff;
    add_header X-XSS-Protection "1; mode=block";
    add_header Strict-Transport-Security "max-age=31536000; includeSubDomains; preload" always;

    location / {
        if ($http_origin ~ '^https?://(localhost|ffplayout\.example\.org)') {
            add_header 'Access-Control-Allow-Origin' "$http_origin" always;
            add_header 'Access-Control-Allow-Credentials' 'true' always;
            add_header 'Access-Control-Allow-Methods' 'GET, POST, PUT, DELETE, OPTIONS' always;
            add_header 'Access-Control-Allow-Headers' 'Accept,Authorization,Cache-Control,Content-Type,DNT,If-Modified-Since,Keep-Alive,Origin,User-Agent,X-Requested-With' always;
        }

        if ($request_method = OPTIONS ) {
            add_header 'Access-Control-Max-Age' 1728000;
            add_header 'Content-Type' 'text/plain charset=UTF-8';
            add_header 'Content-Length' 0;
            return 204;
        }

        root /var/www/ffplayout-frontend/dist/;

    }

    location ~ ^/(api|admin|auth|api-auth) {
        if ($http_origin ~ '^https?://(localhost|ffplayout\.local)') {
            add_header 'Access-Control-Allow-Origin' "$http_origin" always;
            add_header 'Access-Control-Allow-Credentials' 'true' always;
            add_header 'Access-Control-Allow-Methods' 'GET, POST, PUT, DELETE, OPTIONS' always;
            add_header 'Access-Control-Allow-Headers' 'Accept,Authorization,Cache-Control,Content-Type,DNT,If-Modified-Since,Keep-Alive,Origin,User-Agent,X-Requested-With' always;
        }

        add_header Last-Modified $date_gmt;
        add_header Cache-Control 'no-store, no-cache, must-revalidate, proxy-revalidate, max-age=0';
        if_modified_since off;
        expires off;
        etag off;

        proxy_set_header Host $http_host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;
        proxy_read_timeout 36000s;
        proxy_connect_timeout 36000s;
        proxy_send_timeout 36000s;
        proxy_buffer_size 128k;
        proxy_buffers 4 256k;
        proxy_busy_buffers_size 256k;
        send_timeout 36000s;
        proxy_no_cache 1;
        proxy_pass http://127.0.0.1:8001;

    }

    location /static/ {
        alias /var/www/ffplayout-api/ffplayout/static/;
    }

     location /live/ {
        alias /var/www/srs/live/;
    }

    location /preview/stream.flv {
        # HTTP-FLV preview
        proxy_pass http://preview.local:8080/live/stream.flv;
    }
}
```

Of course in production you should have a HTTPS directive to, but this step is up to you.

Restart Nginx.

You you can start ffplayout engine with preview enabled and when you setup everything correct it should run without errors.

You can go now in your frontend configuration and change the `player_url` to: `http://[domain or IP]/preview/stream.flv`, save and reload the page. When you go now to the player tap you should see the preview video.
