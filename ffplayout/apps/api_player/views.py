import os
import shutil
from urllib.parse import unquote

from apps.api_player.models import GuiSettings, MessengePresets
from apps.api_player.serializers import (GuiSettingsSerializer,
                                         MessengerSerializer, UserSerializer)
from django.contrib.auth.models import User
from django_filters import rest_framework as filters
from rest_framework import viewsets
from rest_framework.parsers import FileUploadParser, JSONParser
from rest_framework.response import Response
from rest_framework.views import APIView

from .utils import (PlayoutService, SystemStats, get_media_path, read_json,
                    read_log, read_yaml, send_message, write_json, write_yaml)


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


class MessengerFilter(filters.FilterSet):

    class Meta:
        model = MessengePresets
        fields = ['name']


class MessengerViewSet(viewsets.ModelViewSet):
    queryset = MessengePresets.objects.all()
    serializer_class = MessengerSerializer
    filter_backends = (filters.DjangoFilterBackend,)
    filterset_class = MessengerFilter


class MessageSender(APIView):
    """
    send messages with zmq to the playout engine
    """

    def post(self, request, *args, **kwargs):
        if 'data' in request.data:
            response = send_message(request.data['data'])
            return Response({"success": True, 'status': response})

        return Response({"success": False})


class Config(APIView):
    """
    read and write config from ffplayout engine
    for reading endpoint is: http://127.0.0.1:8000/api/player/config/?config
    """
    parser_classes = [JSONParser]

    def get(self, request, *args, **kwargs):
        if 'configPlayout' in request.GET.dict():
            yaml_input = read_yaml()

            if yaml_input:
                return Response(yaml_input)
            else:
                return Response(status=204)
        else:
            return Response(status=404)

    def post(self, request, *args, **kwargs):
        if 'data' in request.data:
            write_yaml(request.data['data'])
            return Response(status=200)

        return Response(status=404)


class SystemCtl(APIView):
    """
    controlling the ffplayout-engine systemd services
    """

    def post(self, request, *args, **kwargs):
        if 'run' in request.data:
            service = PlayoutService()

            if request.data['run'] == 'start':
                service.start()
                return Response(status=200)
            elif request.data['run'] == 'stop':
                service.stop()
                return Response(status=200)
            elif request.data['run'] == 'reload':
                service.reload()
                return Response(status=200)
            elif request.data['run'] == 'restart':
                service.restart()
                return Response(status=200)
            elif request.data['run'] == 'status':
                status = service.status()
                return Response({"data": status})
            elif request.data['run'] == 'log':
                log = service.log()
                return Response({"data": log})
            else:
                return Response(status=400)

        return Response(status=404)


class LogReader(APIView):
    def get(self, request, *args, **kwargs):
        if 'type' in request.GET.dict() and 'date' in request.GET.dict():
            type = request.GET.dict()['type']
            _date = request.GET.dict()['date']

            log = read_log(type, _date)

            if log:
                return Response({'log': log})
            else:
                return Response(status=204)
        else:
            return Response(status=404)


class Playlist(APIView):
    """
    read and write config from ffplayout engine
    for reading endpoint:
        http://127.0.0.1:8000/api/player/playlist/?date=2020-04-12
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
            return Response(status=400)

    def post(self, request, *args, **kwargs):
        if 'data' in request.data:
            write_json(request.data['data'])
            return Response(status=200)

        return Response(status=400)


class Statistics(APIView):
    """
    get system statistics: cpu, ram, etc.
    for reading, endpoint is: http://127.0.0.1:8000/api/player/stats/?stats=all
    """

    def get(self, request, *args, **kwargs):
        stats = SystemStats()
        if 'stats' in request.GET.dict() and request.GET.dict()['stats'] \
                and hasattr(stats, request.GET.dict()['stats']):
            return Response(
                getattr(stats, request.GET.dict()['stats'])())
        else:
            return Response(status=404)


class Media(APIView):
    """
    get folder/files tree, for building a file explorer
    for reading, endpoint is: http://127.0.0.1:8000/api/player/media/?path
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
                return Response(status=204)
        else:
            return Response(status=404)


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


class FileOperations(APIView):

    def delete(self, request, *args, **kwargs):
        if 'file' in request.GET.dict() and 'path' in request.GET.dict():
            root = read_yaml()['storage']['path']
            _file = unquote(request.GET.dict()['file'])
            folder = unquote(request.GET.dict()['path']).lstrip('/')
            _path = os.path.join(*(folder.split(os.path.sep)[1:]))
            fullPath = os.path.join(root, _path)

            if not _file or _file == 'null':
                if os.path.isdir(fullPath):
                    shutil.rmtree(fullPath, ignore_errors=True)
                    return Response(status=200)
                else:
                    return Response(status=404)
            elif os.path.isfile(os.path.join(fullPath, _file)):
                os.remove(os.path.join(fullPath, _file))
                return Response(status=200)
            else:
                return Response(status=404)
        else:
            return Response(status=404)

    def post(self, request, *args, **kwargs):
        if 'folder' in request.data and 'path' in request.data:
            root = read_yaml()['storage']['path']
            folder = request.data['folder']
            _path = request.data['path'].split(os.path.sep)
            _path = '' if len(_path) == 1 else os.path.join(*_path[1:])
            fullPath = os.path.join(root, _path, folder)

            try:
                # TODO: check if folder exists
                os.mkdir(fullPath)
                return Response(status=200)
            except OSError:
                Response(status=500)
        else:
            return Response(status=404)

    def patch(self, request, *args, **kwargs):
        if 'path' in request.data and 'oldname' in request.data \
                and 'newname' in request.data:
            root = read_yaml()['storage']['path']
            old_name = request.data['oldname']
            new_name = request.data['newname']
            _path = os.path.join(
                *(request.data['path'].split(os.path.sep)[2:]))
            old_file = os.path.join(root, _path, old_name)
            new_file = os.path.join(root, _path, new_name)

            os.rename(old_file, new_file)

            return Response(status=200)
        else:
            return Response(status=204)
