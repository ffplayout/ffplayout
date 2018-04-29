<?php
return array(

    // Basic settings
    'hide_dot_files'            => true,
    'list_folders_first'        => true,
    'list_sort_order'           => 'natcasesort',
    'theme_name'                => 'bootstrap',
    'external_links_new_window' => true,

    // Hidden files
    'hidden_files' => array(
    '.ht*',
    '*/.ht*',
    'resources',
    'resources/*',
    'live',
    'live/*',
	  'functions.php',
	  'process.sh',
    'info.php',
    'ffplayout-gui.png',
    'README.md',
    '.gitignore',
    '.git',
    '.git/*',
    ),

    // Files that, if present in a directory, make the directory
    // a direct link rather than a browse link.
    'index_files' => array(
        'index.htm',
        'index.html',
        'index.php'
    ),

    // Custom sort order
    'reverse_sort' => array(
        // 'path/to/folder'
    ),
);
