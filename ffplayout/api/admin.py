from django.contrib import admin

from api.models import GuiSettings


class GuiSettingsAdmin(admin.ModelAdmin):

    class Meta:
        model = GuiSettings
        fields = '__all__'


admin.site.register(GuiSettings, GuiSettingsAdmin)
