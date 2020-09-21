**We need a recent version of npm**

- clone repo to **/var/www/ffplayout-frontend**
- cd in repo
- install dependencies: `npm install`
- create **.env** file:
    ```
    BASE_URL='http://localhost:3000'
    API_URL='/'
    ```
    - in dev mode `API_URL` should be: `http://localhost:8000`
    - for deactivating progress animation: `DEV=true`
- create symlink for the media folder
    - when your media folder is a subfolder (for example `/opt/ffplayout/media`) create the same folder structure under **static**:
        - `mkdir -p /var/www/ffplayout-frontend/static/opt/ffplayout`
    - `ln -s /opt/ffplayout/media /var/www/ffplayout-frontend/static/opt/ffplayout/`
- build app: `npm run build`

Your frontend should be now in **/var/www/ffplayout-frontend/dist** folder, which we are included already in the nginx config. You can serve now the GUI under your domain URL.

### OS Specific
On debian 10 you need to install:

```
apt install -y curl
```

```
curl -sL https://deb.nodesource.com/setup_12.x | bash -
```

**For full installation (with ffmpeg/srs):**
```
apt install -y sudo net-tools git python3-dev build-essential python3-virtualenv nodejs nginx autoconf automake libtool pkg-config yasm cmake curl mercurial git wget gperf mediainfo
```
