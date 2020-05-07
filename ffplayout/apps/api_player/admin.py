from apps.api_player.models import GuiSettings, MessengePresets
from django.contrib import admin


class GuiSettingsAdmin(admin.ModelAdmin):

    class Meta:
        model = GuiSettings
        fields = '__all__'


class MessengePresetsAdmin(admin.ModelAdmin):
    list_display = ('name',)

    class Meta:
        model = MessengePresets
        fields = '__all__'


admin.site.register(GuiSettings, GuiSettingsAdmin)
admin.site.register(MessengePresets, MessengePresetsAdmin)
