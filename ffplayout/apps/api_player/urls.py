from django.urls import include, path, re_path
from rest_framework import routers

from . import views

router = routers.DefaultRouter()
router.register(r'user/users', views.UserViewSet)
router.register(r'guisettings', views.GuiSettingsViewSet, 'guisettings')
router.register(r'messenger', views.MessengerViewSet, 'messenger')

app_name = 'api_player'

urlpatterns = [
    path('player/', include(router.urls)),
    path('player/config/', views.Config.as_view()),
    path('player/log/', views.LogReader.as_view()),
    path('player/media/', views.Media.as_view()),
    path('player/media/op/', views.FileOperations.as_view()),
    re_path(r'^player/media/upload/(?P<filename>[^/]+)$',
            views.FileUpload.as_view()),
    path('player/messenger/send/', views.MessegeSender.as_view()),
    path('player/playlist/', views.Playlist.as_view()),
    path('player/stats/', views.Statistics.as_view()),
    path('player/user/current/', views.CurrentUserView.as_view()),
    path('player/system/', views.SystemCtl.as_view()),
]
