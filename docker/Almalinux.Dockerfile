FROM almalinux:9 AS base

ENV container docker
RUN dnf -y install libgomp && \
    dnf clean all;

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

ARG FFPLAYOUT_VERSION=0.17.0
COPY README.md *.rpm /tmp/

RUN dnf update -y && \
    dnf install -y epel-release && \
    dnf install -y 'dnf-command(config-manager)' && \
    dnf config-manager --set-enabled crb && \
    dnf install -y --nogpgcheck https://mirrors.rpmfusion.org/free/el/rpmfusion-free-release-$(rpm -E %rhel).noarch.rpm && \
    dnf install -y --nogpgcheck https://mirrors.rpmfusion.org/nonfree/el/rpmfusion-nonfree-release-$(rpm -E %rhel).noarch.rpm && \
    dnf install -y ffmpeg ffmpeg-devel wget dejavu-sans-fonts sudo && \
    dnf clean all

RUN [[ -f /tmp/ffplayout-${FFPLAYOUT_VERSION}-1.x86_64.rpm ]] || wget -q "https://github.com/ffplayout/ffplayout/releases/download/v${FFPLAYOUT_VERSION}/ffplayout-${FFPLAYOUT_VERSION}-1.x86_64.rpm" -P /tmp/ && \
    dnf install -y /tmp/ffplayout-${FFPLAYOUT_VERSION}-1.x86_64.rpm && \
    rm /tmp/ffplayout-${FFPLAYOUT_VERSION}-1.x86_64.rpm && \
    mkdir -p /home/ffpu && chown -R ffpu: /home/ffpu && \
    systemctl enable ffplayout && \
    systemctl enable ffpapi && \
    ffpapi -u admin -p admin -m contact@example.com

EXPOSE 8787

VOLUME [ "/sys/fs/cgroup" ]

CMD ["/usr/sbin/init"]
