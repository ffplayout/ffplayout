Installation
-----

- download latest **..dist** [release](https://github.com/ffplayout/ffplayout-frontend/releases/latest/)
- unpack the dist content to **/var/www/ffplayout-frontend**

- create symlink for the media folder
    - when your media folder is a subfolder (for example `/opt/ffplayout/media`) create the same folder structure under **/var/www/ffplayout-frontend**:
        - `mkdir -p /var/www/ffplayout-frontend/opt/ffplayout`
    - `ln -s /opt/ffplayout/media /var/www/ffplayout-frontend/opt/ffplayout/`

Copy **nginx/ffplayout.conf** to **/etc/nginx/sites-available/** and make a symlink:

`ln -s /etc/nginx/sites-available/ffplayout.conf /etc/nginx/sites-enabled/`

Change the nginx config and add your ssl configuration. After restarting nginx, you should be able to open to frontend in your browser.
