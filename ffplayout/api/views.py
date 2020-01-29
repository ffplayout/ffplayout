import os

from django.conf import settings
from rest_framework.views import APIView
from rest_framework.response import Response

from .utils import IniParser


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
