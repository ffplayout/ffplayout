**We need a recent version of npm**

### OS Specific
On debian 10 you need to install:

```
apt install -y curl
```

```
curl -sL https://deb.nodesource.com/setup_14.x | bash -
```

Installation
-----

- clone repo to **/var/www/ffplayout-frontend**
- cd in repo
- install dependencies: `npm install`
- create **.env** file:
    ```
    BASE_URL='http://example.org'
    API_URL='/'
    ```
    - in dev mode `BASE_URL` should be `http://localhost:3000` and `API_URL=http://localhost:8000`

- create symlink for the media folder
    - when your media folder is a subfolder (for example `/opt/ffplayout/media`) create the same folder structure under **static**:
        - `mkdir -p /var/www/ffplayout-frontend/static/opt/ffplayout`
    - `ln -s /opt/ffplayout/media /var/www/ffplayout-frontend/static/opt/ffplayout/`
- build app: `npm run build`

Your frontend should be now in **/var/www/ffplayout-frontend/dist** folder, which is included in the nginx config.

Copy **docs/ffplayout.conf** to **/etc/nginx/sites-available/** and make a symlink:

`ln -s /etc/nginx/sites-available/ffplayout.conf /etc/nginx/sites-enabled/`

Change the nginx config and add your ssl configuration. After restarting nginx, you should be able to open to frontend in your browser.
