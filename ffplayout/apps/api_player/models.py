import psutil

from django.db import models


class GuiSettings(models.Model):
    """
    Here we manage the settings for the web GUI:
        - Player URL
        - settings for the statistics
    """

    addrs = psutil.net_if_addrs()
    addrs = [(i, i) for i in addrs.keys()]

    player_url = models.CharField(max_length=255)
    playout_config = models.CharField(
        max_length=255,
        default='/etc/ffplayout/ffplayout.yml')
    net_interface = models.CharField(
        max_length=20,
        choices=addrs,
        default=None,
        )
    media_disk = models.CharField(
        max_length=255,
        help_text="should be a mount point, for statistics",
        default='/')
    extra_extensions = models.CharField(
        max_length=255,
        help_text="file extensions, that are only visible in GUI",
        blank=True, null=True, default='')

    def save(self, *args, **kwargs):
        if self.pk is not None or GuiSettings.objects.count() == 0:
            super(GuiSettings, self).save(*args, **kwargs)

    def delete(self, *args, **kwargs):
        if not self.related_query.all():
            super(GuiSettings, self).delete(*args, **kwargs)

    class Meta:
        verbose_name_plural = "guisettings"


class MessengePresets(models.Model):
    name = models.CharField(max_length=255, help_text="the preset name")

    message = models.CharField(
        max_length=1024, blank=True, null=True, default='')

    x = models.CharField(
        max_length=512, blank=True, null=True, default='')

    y = models.CharField(
        max_length=512, blank=True, null=True, default='')

    font_size = models.IntegerField(default=24)
    font_spacing = models.IntegerField(default=4)
    font_color = models.CharField(max_length=12, default='#ffffff')
    font_alpha = models.FloatField(default=1.0)
    show_box = models.BooleanField(default=True)
    box_color = models.CharField(max_length=12, default='#000000')
    box_alpha = models.FloatField(default=0.8)
    border_width = models.IntegerField(default=4)
    overall_alpha = models.CharField(
        max_length=255, blank=True, null=True, default='')

    class Meta:
        verbose_name_plural = "messengepresets"
