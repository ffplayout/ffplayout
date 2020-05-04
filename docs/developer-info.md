### Backend Apps

If you planing to extend the Backend with you own apps (api endpoints),
just add you app folder in **ffplayout/apps**.

If you planing to us a DB, put a **db.py** file in your app. With this `Class`:

```
class Connector:
    config = {
        'default': {
            'ENGINE': 'django.db.backends.sqlite3',
            'NAME': os.path.join(BASE_DIR, 'db-name.sqlite3'),
        }
    }

```
