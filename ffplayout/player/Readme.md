Here you have the possibility to add you own player module. Defaults are: playing a playlist, or the content of a folder.

If you need your own module, create a python file with the desire name. Inside it need a generator class with the name: **GetSourceIter**.

Check **folder.py** and **playlist.py** to get an idea how it needs to work.

After creating the custom module, set in config **play: -> mode:** the file name of your module without extension.
