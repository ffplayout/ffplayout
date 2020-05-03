from api_player.models import GuiSettings
from django.contrib import admin


class GuiSettingsAdmin(admin.ModelAdmin):

    class Meta:
        model = GuiSettings
        fields = '__all__'


admin.site.register(GuiSettings, GuiSettingsAdmin)
