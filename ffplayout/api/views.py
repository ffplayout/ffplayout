# from django.shortcuts import render
from rest_framework.views import APIView
from rest_framework.response import Response

from .utils import IniParser


class Config(APIView):
    def get(self, request, *args, **kwargs):
        if 'config' in request.GET.dict():
            parser = IniParser()
            parser.read('/etc/ffplayout/ffplayout.conf')

            print(dir(parser))
            print(parser.as_dict())

            return Response(parser.as_dict())
        else:
            return Response({"success": False})
