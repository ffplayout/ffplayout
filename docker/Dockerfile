FROM almalinux:9 AS base

ENV container docker

RUN (cd /lib/systemd/system/sysinit.target.wants/; for i in *; do [ $i == \
    systemd-tmpfiles-setup.service ] || rm -f $i; done); \
    rm -f /lib/systemd/system/multi-user.target.wants/*; \
    rm -f /etc/systemd/system/*.wants/*; \
    rm -f /lib/systemd/system/local-fs.target.wants/*; \
    rm -f /lib/systemd/system/sockets.target.wants/*udev*; \
    rm -f /lib/systemd/system/sockets.target.wants/*initctl*; \
    rm -f /lib/systemd/system/basic.target.wants/*; \
    rm -f /lib/systemd/system/anaconda.target.wants/*

FROM base

ARG FFPLAYOUT_VERSION=0.22.0
COPY README.md *.rpm /tmp/

RUN dnf update -y && \
    dnf install -y epel-release && \
    dnf install -y 'dnf-command(config-manager)' && \
    dnf config-manager --set-enabled crb && \
    dnf install -y --nogpgcheck https://mirrors.rpmfusion.org/free/el/rpmfusion-free-release-$(rpm -E %rhel).noarch.rpm && \
    dnf install -y --nogpgcheck https://mirrors.rpmfusion.org/nonfree/el/rpmfusion-nonfree-release-$(rpm -E %rhel).noarch.rpm && \
    dnf install -y ffmpeg wget dejavu-sans-fonts sudo && \
    dnf clean all

RUN [[ -f /tmp/ffplayout-${FFPLAYOUT_VERSION}-1.x86_64.rpm ]] || wget -q "https://github.com/ffplayout/ffplayout/releases/download/v${FFPLAYOUT_VERSION}/ffplayout-${FFPLAYOUT_VERSION}-1.x86_64.rpm" -P /tmp/ && \
    dnf install -y /tmp/ffplayout-${FFPLAYOUT_VERSION}-1.x86_64.rpm && \
    rm /tmp/ffplayout-${FFPLAYOUT_VERSION}-1.x86_64.rpm && \
    sed -i "s/User=ffpu/User=root/g" /usr/lib/systemd/system/ffplayout.service && \
    systemctl enable ffplayout && \
    ffplayout -u admin -p admin -m contact@example.com

EXPOSE 8787

# Maybe on some systems is needed, combined with run parameters: --tmpfs /tmp --tmpfs /run --tmpfs /run/lock
# More infos: https://serverfault.com/a/1087467/387878
#VOLUME [ "/tmp", "/run", "/run/lock" ]

CMD ["/usr/sbin/init"]
