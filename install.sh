#!/usr/bin/env bash

if [[ $(whoami) != 'root' ]]; then
    echo "This script must run under root!"
    exit 1
fi

if [ ! "$(grep -Ei 'debian|buntu|mint' /etc/*release)" ]; then
    echo "This script must run under debian/ubuntu/mint!"
    exit 1
fi

echo ""
echo "-----------------------------------------------------------------------------------------------------"
echo "compile and install (nonfree) ffmpeg:"
echo "-----------------------------------------------------------------------------------------------------"
echo ""
while true; do
    read -p "Do you wish to compile ffmpeg? (Y/n) :$ " yn
    case $yn in
        [Yy]* ) compileFFmpeg="y"; break;;
        [Nn]* ) compileFFmpeg="n"; break;;
        * ) (
            echo "------------------------------------"
            echo "Please answer yes or no!"
            echo ""
            );;
    esac
done

echo ""
echo "-----------------------------------------------------------------------------------------------------"
echo "install and setup nginx:"
echo "-----------------------------------------------------------------------------------------------------"
echo ""
while true; do
    read -p "Do you wish to install nginx? (Y/n) :$ " yn
    case $yn in
        [Yy]* ) installNginx="y"; break;;
        [Nn]* ) installNginx="n"; break;;
        * ) (
            echo "------------------------------------"
            echo "Please answer yes or no!"
            echo ""
            );;
    esac
done

echo ""
echo "-----------------------------------------------------------------------------------------------------"
echo "install and srs rtmp/hls server:"
echo "-----------------------------------------------------------------------------------------------------"
echo ""
while true; do
    read -p "Do you wish to install srs? (Y/n) :$ " yn
    case $yn in
        [Yy]* ) installSRS="y"; break;;
        [Nn]* ) installSRS="n"; break;;
        * ) (
            echo "------------------------------------"
            echo "Please answer yes or no!"
            echo ""
            );;
    esac
done

echo ""
echo "-----------------------------------------------------------------------------------------------------"
echo "install main packages"
echo "-----------------------------------------------------------------------------------------------------"

apt install -y sudo curl wget net-tools git python3-dev build-essential virtualenv python3-virtualenv mediainfo

curl -sL https://deb.nodesource.com/setup_12.x | bash -

apt install -y nodejs

if [[ $installNginx == 'y' ]]; then
    apt install -y nginx
fi

if [[ $compileFFmpeg == 'y' ]]; then
    echo ""
    echo "-----------------------------------------------------------------------------------------------------"
    echo "compile and install ffmpeg"
    echo "-----------------------------------------------------------------------------------------------------"
    apt install -y autoconf automake libtool pkg-config texi2html yasm cmake mercurial gperf

    cd /opt/

    git clone https://github.com/jb-alvarado/compile-ffmpeg-osx-linux.git ffmpeg-build

    cd ffmpeg-build

cat <<EOF > "build_config.txt"
#--enable-decklink
--disable-ffplay
--disable-sdl2
--enable-fontconfig
#--enable-libaom
#--enable-libass
#--enable-libbluray
--enable-libfdk-aac
--enable-libfribidi
--enable-libfreetype
--enable-libmp3lame
--enable-libopus
--enable-libsoxr
--enable-libsrt
--enable-libtwolame
--enable-libvpx
--enable-libx264
--enable-libx265
--enable-libzimg
--enable-libzmq
--enable-nonfree
#--enable-opencl
#--enable-opengl
#--enable-openssl
#--enable-libsvtav1
EOF
    sed -i 's/mediainfo="yes"/mediainfo="no"/g' ./compile-ffmpeg.sh
    sed -i 's/mp4box="yes"/mp4box="no"/g' ./compile-ffmpeg.sh

    ./compile-ffmpeg.sh

    cp local/bin/ffmpeg /usr/local/bin/
    cp local/bin/ffprobe /usr/local/bin/
fi

if [[ $installSRS == 'y' ]]; then
    echo ""
    echo "-----------------------------------------------------------------------------------------------------"
    echo "compile and install srs"
    echo "-----------------------------------------------------------------------------------------------------"

    cd /opt/
    git clone https://github.com/ossrs/srs.git
    cd srs/trunk/

    ./configure
    ./make
    ./make install

    mkdir -p "/var/www/srs/live"
    mkdir "/etc/srs"

cat <<EOF > "/etc/srs/srs.conf"
listen              1935;
max_connections     20;
daemon              on;
pid                 /usr/local/srs/objs/srs.pid;
srs_log_tank        console; # file;
srs_log_file        /var/log/srs.log;
ff_log_dir          /tmp;

# can be: verbose, info, trace, warn, error
srs_log_level       error;

http_api {
    enabled         on;
    listen          1985;
}

stats {
    network         0;
    disk            sda vda xvda xvdb;
}

vhost __defaultVhost__ {
    # timestamp correction
    mix_correct     on;

    http_hooks {
        enabled         off;
        on_publish      http://127.0.0.1:8085/api/v1/streams;
        on_unpublish    http://127.0.0.1:8085/api/v1/streams;
    }

    hls {
        enabled         on;
        hls_path        /var/www/srs;
        hls_fragment    6;
        hls_window      3600;
        hls_cleanup     on;
        hls_dispose     0;
        hls_m3u8_file   live/stream.m3u8;
        hls_ts_file     live/stream-[seq].ts;
    }
}
EOF

cat <<EOF > "/etc/systemd/system/srs.service"
[Unit]
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

EOF

    systemctl enable srs.service
    systemctl start srs.service
fi

echo ""
echo "-----------------------------------------------------------------------------------------------------"
echo "install ffplayout engine"
echo "-----------------------------------------------------------------------------------------------------"

cd /opt
git clone https://github.com/ffplayout/ffplayout-engine.git
cd ffplayout-engine

virtualenv -p python3 venv
source ./venv/bin/activate

pip install -r requirements-base.txt

mkdir /etc/ffplayout
mkdir /var/log/ffplayout

cp ffplayout.yml /etc/ffplayout/
chown -R www-data. /etc/ffplayout
chown www-data. /var/log/ffplayout

cp docs/ffplayout-engine.service /etc/systemd/system/
sed -i "s/User=root/User=www-data/g" /etc/systemd/system/ffplayout-engine.service
sed -i "s/Group=root/Group=www-data/g" /etc/systemd/system/ffplayout-engine.service

systemctl enable ffplayout-engine.service

deactivate

echo ""
echo "-----------------------------------------------------------------------------------------------------"
echo "install ffplayout gui"
echo "-----------------------------------------------------------------------------------------------------"

cd /var/www
git clone https://github.com/ffplayout/ffplayout-gui.git ffplayout
cd ffplayout

virtualenv -p python3 venv
source ./venv/bin/activate

pip install -r requirements-base.txt

cd ffplayout

secret=$(python manage.py shell -c 'from django.core.management import utils; print(utils.get_random_secret_key())')

sed -i "s/---a-very-important-secret-key:-generate-it-new---/$secret/g" ffplayout/settings/production.py

python manage.py makemigrations && python manage.py migrate
python manage.py collectstatic
python manage.py loaddata ../docs/db_data.json
python manage.py createsuperuser

deactivate

chown www-data. -R /var/www/ffplayout

cd ..

cp docs/ffplayout-api.service /etc/systemd/system/
systemctl enable ffplayout-api.service && systemctl start ffplayout-api.service

cp  docs/ffplayout.conf /etc/nginx/sites-available/
ln -s /etc/nginx/sites-available/ffplayout.conf /etc/nginx/sites-enabled/

echo 'www-data  ALL = NOPASSWD: /bin/systemctl start ffplayout-engine.service, /bin/systemctl stop ffplayout-engine.service, /bin/systemctl reload ffplayout-engine.service, /bin/systemctl restart ffplayout-engine.service, /bin/systemctl status ffplayout-engine.service, /bin/systemctl is-active ffplayout-engine.service, /bin/journalctl -n 1000 -u ffplayout-engine.service' >> /etc/sudoers

cd /var/www/ffplayout/ffplayout/frontend

npm install

cat <<EOF > ".env"
BASE_URL='http://localhost:3000'
API_URL='/'
EOF

npm run build

echo ""
echo "-----------------------------------------------------------------------------------------------------"
echo "installation done..."
echo "-----------------------------------------------------------------------------------------------------"

echo "please edit /var/www/ffplayout/ffplayout/ffplayout/settings/production.py"
echo "and set ALLOWED_HOSTS and CORS_ORIGIN_WHITELIST"
echo ""
echo "edit /etc/nginx/sites-available/ffplayout.conf"
echo "set server_name and http_origin"
echo ""
echo "add your ssl config!"
