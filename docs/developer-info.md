### Backend Apps

If you planing to extend the backend with your own apps (api endpoints),
just add your app in folder: **ffplayout/apps**.

If you planing to us a DB, put a **settings.py** file in your app. With this object:

```python
DATABASES = {
    'default': {
        'ENGINE': 'django.db.backends.sqlite3',
        'NAME': os.path.join(BASE_DIR, 'db-name.sqlite3'),
    }
}
```
