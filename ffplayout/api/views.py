import os

from django.conf import settings
from rest_framework.views import APIView
from rest_framework.response import Response

from .utils import IniParser, SystemStats


class Config(APIView):
    """
    read and write config from ffplayout engine
    for reading, endpoint is: http://127.0.0.1:8000/api/config/?config
    """

    def get(self, request, *args, **kwargs):
        if 'config' in request.GET.dict():
            if os.path.isfile(settings.FFPLAYOUT_CONFIG):
                parser = IniParser()
                parser.read(settings.FFPLAYOUT_CONFIG)

                return Response(parser.as_dict())
            else:
                return Response({
                    "success": False,
                    "error": "ffpayout engine config file not found!"})
        else:
            return Response({"success": False})


class Statistics(APIView):
    """
    get system statistics: cpu, ram, etc.
    for reading, endpoint is: http://127.0.0.1:8000/api/stats/?stats=all
    """

    def get(self, request, *args, **kwargs):
        if 'stats' in request.GET.dict() \
                and request.GET.dict()['stats'] == 'all':
            return Response(SystemStats().all())
        else:
            return Response({"success": False})
