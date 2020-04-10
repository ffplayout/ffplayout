from rest_framework.response import Response
from rest_framework.views import APIView

from .utils import read_yaml, write_yaml, SystemStats, get_media_path


class Config(APIView):
    """
    read and write config from ffplayout engine
    for reading endpoint is: http://127.0.0.1:8000/api/config/?config
    """

    def get(self, request, *args, **kwargs):
        if 'config' in request.GET.dict():
            yaml_input = read_yaml()

            if yaml_input:
                return Response(yaml_input)
            else:
                return Response({
                    "success": False,
                    "error": "ffpayout engine config file not found!"})
        else:
            return Response({"success": False})

    def post(self, request, *args, **kwargs):
        if 'data' in request.data:
            write_yaml(request.data['data'])
            return Response({"success": True})

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
