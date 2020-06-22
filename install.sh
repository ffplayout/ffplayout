#!/usr/bin/env bash

if [[ $(whoami) != 'root' ]]; then
    echo "This script must run under root!"
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

if [[ "$(grep -Ei 'debian|buntu|mint' /etc/*release)" ]]; then
    apt install -y sudo curl wget net-tools git python3-dev build-essential python3-virtualenv mediainfo autoconf automake libtool pkg-config yasm cmake mercurial gperf
    curl -sL https://deb.nodesource.com/setup_12.x | bash -

    apt install -y nodejs

    if [[ $installNginx == 'y' ]]; then
        apt install -y nginx
    fi

    serviceUser="www-data"
    nginxConfig="/etc/nginx/sites-available/"
elif [[ "$(grep -Ei 'centos|fedora' /etc/*release)" ]]; then
    dnf -y install epel-release
    dnf repolist epel -v
    dnf -y config-manager --enable PowerTools
    dnf -y group install "Development Tools"
    dnf -y --enablerepo=PowerTools install libmediainfo mediainfo
    dnf -y install libstdc++-static yasm mercurial libtool cmake net-tools git python3 python36-devel wget python3-virtualenv gperf nano
    dnf -y install policycoreutils-{python3,devel}

    curl -sL https://rpm.nodesource.com/setup_12.x | sudo -E bash -

    dnf -y install nodejs

    if [[ $installNginx == 'y' ]]; then
        dnf -y install nginx
        systemctl enable nginx
        systemctl start nginx
        firewall-cmd --permanent --add-service=http
        firewall-cmd --permanent --zone=public --add-service=https
        firewall-cmd --reload
        mkdir /var/www
        chcon -vR system_u:object_r:httpd_sys_content_t:s0 /var/www
    fi

    alternatives --set python /usr/bin/python3

    serviceUser="nginx"
    nginxConfig="/etc/nginx/conf.d/"
fi



if [[ $compileFFmpeg == 'y' ]]; then
    echo ""
    echo "-----------------------------------------------------------------------------------------------------"
    echo "compile and install ffmpeg"
    echo "-----------------------------------------------------------------------------------------------------"
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
    make
    make install

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
chown -R $serviceUser. /etc/ffplayout
chown $serviceUser. /var/log/ffplayout

cp docs/ffplayout-engine.service /etc/systemd/system/
sed -i "s/User=root/User=$serviceUser/g" /etc/systemd/system/ffplayout-engine.service
sed -i "s/Group=root/Group=$serviceUser/g" /etc/systemd/system/ffplayout-engine.service

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

chown $serviceUser. -R /var/www/ffplayout

cd ..

cp docs/ffplayout-api.service /etc/systemd/system/

sed -i "s/User=root/User=$serviceUser/g" /etc/systemd/system/ffplayout-api.service
sed -i "s/Group=root/Group=$serviceUser/g" /etc/systemd/system/ffplayout-api.service

systemctl enable ffplayout-api.service && systemctl start ffplayout-api.service

if [[ "$(grep -Ei 'debian|buntu|mint' /etc/*release)" ]]; then
    cp docs/ffplayout.conf "$nginxConfig"
    ln -s $nginxConfig/ffplayout.conf /etc/nginx/sites-enabled/
elif [[ "$(grep -Ei 'centos|fedora' /etc/*release)" ]]; then
    cp docs/ffplayout.conf "$nginxConfig"

    setsebool httpd_can_network_connect on -P
    semanage port -a -t http_port_t -p tcp 8001

cat <<EOF > gunicorn.te
module gunicorn 1.0;

require {
        type init_t;
        type httpd_sys_content_t;
        type etc_t;
        type sudo_exec_t;
        class file { create execute execute_no_trans getattr ioctl lock map open read unlink write };
        class lnk_file { getattr read };
}

#============= init_t ==============

#!!!! This avc is allowed in the current policy
allow init_t etc_t:file write;

#!!!! This avc is allowed in the current policy
#!!!! This av rule may have been overridden by an extended permission av rule
allow init_t httpd_sys_content_t:file { create execute execute_no_trans getattr ioctl lock map open read unlink write };

#!!!! This avc is allowed in the current policy
allow init_t httpd_sys_content_t:lnk_file { getattr read };

#!!!! This avc is allowed in the current policy
allow init_t sudo_exec_t:file { execute execute_no_trans map open read };
EOF

    checkmodule -M -m -o gunicorn.mod gunicorn.te
    semodule_package -o gunicorn.pp -m gunicorn.mod
    semodule -i gunicorn.pp

cat <<EOF > conf.te
module conf 1.0;

require {
        type init_t;
        type httpd_sys_content_t;
        class file { create lock unlink write };
}

#============= init_t ==============
allow init_t httpd_sys_content_t:file unlink;

#!!!! This avc is allowed in the current policy
allow init_t httpd_sys_content_t:file { create lock write };
EOF

    checkmodule -M -m -o conf.mod conf.te
    semodule_package -o conf.pp -m conf.mod
    semodule -i conf.pp
fi


echo "$serviceUser  ALL = NOPASSWD: /bin/systemctl start ffplayout-engine.service, /bin/systemctl stop ffplayout-engine.service, /bin/systemctl reload ffplayout-engine.service, /bin/systemctl restart ffplayout-engine.service, /bin/systemctl status ffplayout-engine.service, /bin/systemctl is-active ffplayout-engine.service, /bin/journalctl -n 1000 -u ffplayout-engine.service" >> /etc/sudoers

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
echo "edit $nginxConfig/ffplayout.conf"
echo "set server_name and http_origin"
echo ""
echo "add your ssl config, reboot and login to the webapp"
