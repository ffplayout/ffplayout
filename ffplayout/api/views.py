import os
from urllib.parse import unquote

from django.contrib.auth.models import User
from django_filters import rest_framework as filters
from rest_framework import viewsets
from rest_framework.parsers import FileUploadParser, JSONParser
from rest_framework.response import Response
from rest_framework.views import APIView

from api.models import GuiSettings
from api.serializers import GuiSettingsSerializer, UserSerializer

from .utils import (SystemStats, get_media_path, read_json, read_yaml,
                    write_yaml)


class CurrentUserView(APIView):
    def get(self, request):
        serializer = UserSerializer(request.user)
        return Response(serializer.data)


class UserFilter(filters.FilterSet):

    class Meta:
        model = User
        fields = ['username']


class UserViewSet(viewsets.ModelViewSet):
    queryset = User.objects.all()
    serializer_class = UserSerializer
    filter_backends = (filters.DjangoFilterBackend,)
    filterset_class = UserFilter


class GuiSettingsViewSet(viewsets.ModelViewSet):
    """
    API endpoint that allows media to be viewed.
    """
    queryset = GuiSettings.objects.all()
    serializer_class = GuiSettingsSerializer


class Config(APIView):
    """
    read and write config from ffplayout engine
    for reading endpoint is: http://127.0.0.1:8000/api/config/?config
    """
    parser_classes = [JSONParser]

    def get(self, request, *args, **kwargs):
        if 'configPlayout' in request.GET.dict():
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


class Playlist(APIView):
    """
    read and write config from ffplayout engine
    for reading endpoint: http://127.0.0.1:8000/api/playlist/?date=2020-04-12
    """

    def get(self, request, *args, **kwargs):
        if 'date' in request.GET.dict():
            date = request.GET.dict()['date']
            json_input = read_json(date)

            if json_input:
                return Response(json_input)
            else:
                return Response({
                    "success": False,
                    "error": "Playlist from {} not found!".format(date)})
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
        stats = SystemStats()
        if 'stats' in request.GET.dict() and request.GET.dict()['stats'] \
                and hasattr(stats, request.GET.dict()['stats']):
            return Response(
                getattr(stats, request.GET.dict()['stats'])())
        else:
            return Response({"success": False})


class Media(APIView):
    """
    get folder/files tree, for building a file explorer
    for reading, endpoint is: http://127.0.0.1:8000/api/media/?path
    """

    def get(self, request, *args, **kwargs):
        if 'extensions' in request.GET.dict():
            extensions = request.GET.dict()['extensions']

            if 'path' in request.GET.dict() and request.GET.dict()['path']:
                return Response({'tree': get_media_path(
                    extensions, request.GET.dict()['path']
                )})
            elif 'path' in request.GET.dict():
                return Response({'tree': get_media_path(extensions)})
            else:
                return Response({"success": False})
        else:
            return Response({"success": False})


class FileUpload(APIView):
    parser_classes = [FileUploadParser]

    def put(self, request, filename, format=None):
        root = read_yaml()['storage']['path']
        file_obj = request.data['file']
        filename = unquote(filename)
        path = unquote(request.query_params['path']).split('/')[1:]

        with open(os.path.join(root, *path, filename), 'wb') as outfile:
            for chunk in file_obj.chunks():
                outfile.write(chunk)
        return Response(status=204)
