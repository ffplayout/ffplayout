# Run ffplayout in container


## Base Image

Use of [CentOS image](https://hub.docker.com/_/centos) as base image as it offer the possibility to use systemd.
In order to run systemd in a container it has to run in privileged mode and bind to the `cgroup` of the host.

> **_NOTE:_**  A system with CGroup V2 is need!
> Currently tested host systems are:
> - debian 12 with official docker ce from docker.com
> - Manjaro from 2024 Kernel 6.6+
> - fedora 39 with podman

## Image

In addition to the base image, there is the compilation of ffmpeg and all lib from source based on https://github.com/jrottenberg/ffmpeg.
We can't use directly the image from `jrottenberg/ffmpeg` as it compile ffmpeg with the flag `--enable-small` that remove some part of the json from the ffprobe command.

The image is build with a default user/pass `admin/admin`.

You can take a look at the [Dockerfile](Dockerfile)

### /!\ as ffmpeg is compiled with `--enable-nonfree` don't push it to a public registry nor distribute the image /!\

## Storage

There are some folders/files that are important for ffplayout to work well such as:
 - **/usr/share/ffplayout/db** => where all the data are stored (user/pass etc)
 - **/var/lib/ffplayout/tv-media** => where the media are stored by default (configurable)
 - **/var/lib/ffplayout/playlists** => where playlists are stored (configurable)
 - **/etc/ffplayout/ffplayout.yml** => the core config file

It may be useful to create/link volume for those folders/files.

## Docker

How to build the image:\
```BASH
# build default
docker build -t ffplayout-image .

# build from root folder, to copy local *.rpm package
docker build -f docker/Dockerfile -t ffplayout-image:alma .

# build ffmpeg from source
docker build -f fromSource.Dockerfile -t ffplayout-image:from-source .

# build with nvidia image for hardware support
docker build -f nvidia-centos7.Dockerfile -t ffplayout-image:nvidia .
```

example of command to start the container:

```BASH
docker run -it --name ffplayout --privileged -p 8787:8787 ffplayout-image

# run in daemon mode
docker run -d --name ffplayout --privileged -p 8787:8787 ffplayout-image

# run with docker-compose
docker-compose up -d
```

#### Note from CentOS docker hub page
There have been reports that if you're using an Ubuntu host, you will need to add `-v /tmp/$(mktemp -d):/run` to the mount.

## Kubernetes

basic example to run the service in k8s:
```
---
apiVersion: apps/v1
kind: Deployment
metadata:
  labels:
    app: ffplayout
  name: ffplayout
  namespace: ffplayout
spec:
  replicas: 1
  selector:
    matchLabels:
      app: ffplayout
  strategy:
    type: Recreate
  template:
    metadata:
      labels:
        app: ffplayout
    spec:
      containers:
      - name: ffplayout
        securityContext:
          allowPrivilegeEscalation: true
          capabilities:
            add:
            - SYS_ADMIN
        image: ffplayout-image:latest
        ports:
        - containerPort: 8787
          name: web
          protocol: TCP
        volumeMounts:
          - name: cgroup
            mountPath: /sys/fs/cgroup
            readOnly: true
          - name: database-volume
            mountPath: /usr/share/ffplayout/db
      restartPolicy: Always
      volumes:
      - name: cgroup
        hostPath:
          path: '/sys/fs/cgroup'
          type: Directory
      - name: database-volume
        ephemeral:
          volumeClaimTemplate:
            metadata:
              labels:
                type: my-database-volume
            spec:
              accessModes: [ "ReadWriteOnce" ]
              storageClassName: "database-storage-class"
              resources:
                requests:
                  storage: 1Gi
```



### Use with traefik

If you are using traefik here is a sample config
```
---
kind: Service
apiVersion: v1
metadata:
  name: ffplayout
  namespace: ffplayout
spec:
  ports:
  - port: 8787
    name: web
    protocol: TCP
  selector:
    app: ffplayout
---
apiVersion: traefik.containo.us/v1alpha1
kind: IngressRoute
metadata:
  name: ffplayout-http
  namespace: ffplayout
spec:
  entryPoints:
    - web
  routes:
  - match: Host(`ffplayout.example.com`) && PathPrefix(`/`)
    kind: Rule
    middlewares:
    - name: redirect-https
      namespace: default
    services:
    - name: ffplayout
      namespace: ffplayout
      port: 8787
---
apiVersion: traefik.containo.us/v1alpha1
kind: IngressRoute
metadata:
  name: ffplayout-https
  namespace: ffplayout
spec:
  entryPoints:
    - websecure
  routes:
  - match: Host(`ffplayout.example.com`) && PathPrefix(`/`)
    kind: Rule
    services:
    - name: ffplayout
      namespace: ffplayout
      port: 8787
  tls:
    certResolver: yourCert
```
