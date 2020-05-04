import os

BASE_DIR = os.path.dirname(os.path.abspath(os.path.join(__file__)))

class Connector:
    config = {
        'default': {
            'ENGINE': 'django.db.backends.sqlite3',
            'NAME': os.path.join(BASE_DIR, 'db.sqlite3'),
        }
    }
