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
    extra_extensions =  models.CharField(
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
