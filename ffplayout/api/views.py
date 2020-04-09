import os

import yaml

from django.conf import settings
from rest_framework.response import Response
from rest_framework.views import APIView
from rest_framework.permissions import AllowAny

from .utils import SystemStats, get_media_path


def read_config(path):
    with open(path, 'r') as config_file:
        return yaml.safe_load(config_file)


class Config(APIView):
    permission_classes = (AllowAny,)
    """
    read and write config from ffplayout engine
    for reading endpoint is: http://127.0.0.1:8000/api/config/?config
    """

    def get(self, request, *args, **kwargs):
        if 'config' in request.GET.dict():
            if os.path.isfile(settings.FFPLAYOUT_CONFIG):
                yaml = read_config(settings.FFPLAYOUT_CONFIG)
                return Response(yaml)
            else:
                return Response({
                    "success": False,
                    "error": "ffpayout engine config file not found!"})
        else:
            return Response({"success": False})

    def post(self, request, *args, **kwargs):
        if 'config' in request.POST.dict():
            pass
        print(request.query_params)
        print(args)
        print(kwargs)

        return Response({"success": False})


class Statistics(APIView):
    """
    get system statistics: cpu, ram, etc.
    for reading, endpoint is: http://127.0.0.1:8000/api/stats/?stats=all
    """

    def get(self, request, *args, **kwargs):
        if 'stats' in request.GET.dict() and request.GET.dict()['stats'] \
                and hasattr(SystemStats(), request.GET.dict()['stats']):
            return Response(
                getattr(SystemStats(), request.GET.dict()['stats'])())
        else:
            return Response({"success": False})


class Media(APIView):
    """
    get folder/files tree, for building a file explorer
    for reading, endpoint is: http://127.0.0.1:8000/api/media/?path
    """

    def get(self, request, *args, **kwargs):
        if 'path' in request.GET.dict() and request.GET.dict()['path']:
            return Response({'tree': get_media_path(
                request.GET.dict()['path']
            )})
        elif 'path' in request.GET.dict():
            return Response({'tree': get_media_path()})
        else:
            return Response({"success": False})
