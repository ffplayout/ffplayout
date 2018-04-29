<?php

/* -----------------------------------------------------------------------------
read values from ffplayout config file
------------------------------------------------------------------------------*/

// get config file
function get_config() {
    return file_get_contents("/etc/ffplayout/ffplayout.conf");
}

// get start time
if(!empty($_GET['list_start'])) {
    $get_ini = get_config();
    $get_start = "/^day_start.*\$/m";
    preg_match_all($get_start, $get_ini, $matches);
    $line = implode("\n", $matches[0]);
    echo explode("= ", $line)[1];
}

// get clips root directory
if(!empty($_GET['clips_root'])) {
    $get_ini = get_config();
    $get_root = "/^clips_root.*\$/m";
    preg_match_all($get_root, $get_ini, $matches);
    $line = implode("\n", $matches[0]);
    $root = substr(explode("= ", $line)[1], 1);
    echo $root;
}

/* -----------------------------------------------------------------------------
xml playlist operations
------------------------------------------------------------------------------*/

// file extension to filter
// for make it nicer
$except = array(
    'avi',
    'mp4',
    'mov',
    'mkv',
    'mpg',
    'mpeg',
    );

$ext = implode('|', $except);

// read from xml file
// formate the values and generate readeble output
if(!empty($_GET['xml_path'])) {
    $xml_date = $_GET['xml_path'];
    $date_str = explode('-', $xml_date);
    $get_ini = get_config();
    $get_dir = "/^playlist_path.*\$/m";
    preg_match_all($get_dir, $get_ini, $matches);
    $line = implode("\n", $matches[0]);
    $path_root = explode("= ", $line)[1];

    $xml_path = $path_root . "/" . $date_str[0] . "/" . $date_str[1] . "/" . $xml_date . ".xml";

    if (file_exists($xml_path)) {
        $xml = simplexml_load_file($xml_path) or die("Error: Cannot create object");

        $src_re = array();
        $src_re[0] = '/# [0-9-]+.('.$ext.')$/';
        $src_re[1] = '/^[0-9]+ # /';
        $src_re[2] = '/.('.$ext.')$/';
        $src_re[3] = '/^# /';

        foreach($xml->body[0]->video as $video) {
            $src       = preg_replace('/^\//', '', $video['src']);
            $src_arr   = explode('/', $src);
            $name      = preg_replace($src_re, '', end($src_arr));
            $name      = str_replace('§', '?', $name);
            $clipBegin = $video['begin'];
            $begin     = gmdate("H:i:s", intval($clipBegin));
            $dur       = $video['dur'];
            $duration  = gmdate("H:i:s", intval($dur));
            $in        = $video['in'];
            $in_p      = gmdate("H:i:s", intval($in));
            $out       = $video['out'];
            $out_p     = gmdate("H:i:s", intval($out));

            $play      = '<a href="#" data-href="' .  $src . '" class="file-play"><i class="fa fa-play-circle file-op"></i></a>';

            printf('<li class="list-item" begin="%s" src="%s" dur="%s" in="%s" out="%s">
             <ul class="inner-item">
               <li class="row-start">%s</li>
               <li class="row-file">%s</li>
               <li class="row-preview">%s</li>
               <li class="row-duration">%s</li>
               <li class="row-in"><input type="text" class="input-in" name="seek_in" value="%s"></li>
               <li class="row-out"><input type="text" class="input-out" name="cut_end" value="%s"></li>
               <li class="row-del"><i class="fa fa-times-circle-o file-op"></i></li>
             </ul>
             <i class="handle"></i>
             </li>
             ',
            $clipBegin, $src, $dur, $in, $out,
            $begin, $name, $play, $duration, $in_p, $out_p
            );
        }
    } else {
        printf('<li class="list-item" begin="0.0" src="None" dur="0.0" in="0.0" out="0.0">
         <ul class="inner-item">
           <li class="row-start">%s</li>
           <li class="row-file">%s</li>
           <li class="row-preview">%s</li>
           <li class="row-duration">%s</li>
           <li class="row-in">%s</li>
           <li class="row-out">%s</li>
           <li class="row-del">%s</li>
         </ul>
         <i class="handle"></i>
         </li>
         ',
        "...", "No Playlist for this Day", "...", "...", "...", "...", "..."
        );
    }
}

// generate object from dragged item
if(!empty($_GET['li_path'])) {
    $path = urldecode($_GET['li_path']);

    $src_re = array();
    $src_re[0] = '/# [0-9-]+.('.$ext.')$/';
    $src_re[1] = '/^[0-9]+ # /';
    $src_re[2] = '/.('.$ext.')$/';
    $src_re[3] = '/^# /';

    $src       = preg_replace('/^\//', '', $path);
    $src_arr   = explode('/', $src);
    $name      = preg_replace($src_re, '', end($src_arr));
    $play      = '<a href="#" data-href="' .  $src . '" class="file-play"><i class="fa fa-play-circle file-op"></i></a>';
    $duration  = preg_replace("/\n\n|\n|\n/",'',shell_exec("ffprobe -v error -show_entries format=duration -of default=noprint_wrappers=1:nokey=1 '".$path."'"));
    $dur_time  = gmdate("H:i:s", $duration);

    printf('<li class="list-item" begin="%s" src="%s" dur="%s" in="%s" out="%s">
     <ul class="inner-item">
       <li class="row-start">%s</li>
       <li class="row-file">%s</li>
       <li class="row-preview">%s</li>
       <li class="row-duration">%s</li>
       <li class="row-in"><input type="text" class="input-in" name="seek_in" value="%s"></li>
       <li class="row-out"><input type="text" class="input-out" name="cut_end" value="%s"></li>
       <li class="row-del"><i class="fa fa-times-circle-o file-op"></i></li>
     </ul>
     <i class="handle"></i>
     </li>
     ',
    "0", $src, $duration, "0", $duration,
    "00:00:00", $name, $play, $dur_time, "00:00:00", $dur_time
    );
}

// save modified list
if(!empty($_POST['save'])) {
    // get json string
    $raw_arr = json_decode(urldecode($_POST['save']));
    $date = $_POST['date'];
    $date_str = explode('-', $date);
    // get save path
    $get_ini = get_config();
    $get_dir = "/^playlist_path.*\$/m";
    preg_match_all($get_dir, $get_ini, $matches);
    $line = implode("\n", $matches[0]);
    $path_root = explode("= ", $line)[1];
    $xml_path = $path_root . "/" . $date_str[0] . "/" . $date_str[1];
    $xml_output = $xml_path . "/" . $date . ".xml";

    // create xml head
    $xml_str = sprintf('<playlist>
    <head>
        <meta name="author" content="example"/>
        <meta name="title" content="Live Stream"/>
        <meta name="copyright" content="(c)%s example.org"/>
        <meta name="date" content="%s"/>
    </head>
    <body>%s', $date_str[0], $date, "\n");

    // create xml video element
    foreach($raw_arr as $rawline) {
        $formated_src = str_replace('&', '&amp;', $rawline->src);
        $xml_str .= sprintf('        <video src="%s" begin="%s" dur="%s" in="%s" out="%s"/>%s', $formated_src, $rawline->begin, $rawline->dur, $rawline->in, $rawline->out, "\n");
    }

    // crate xml end
    $xml_str .= "    </body>\n</playlist>\n";

    if (!is_dir($xml_path)) {
        mkdir($xml_path, 0777, true);
    }

    file_put_contents($xml_output, $xml_str);
    printf('Save playlist "%s.xml" done...', $date);
}

// fill playlist to 24 hours
if(!empty($_POST['fill_playlist'])) {
    $list_date = $_POST['fill_playlist'];
    $diff_len = $_POST['diff_len'];
    $start_time = $_POST['start_time'];
    $raw_arr = json_decode(urldecode($_POST['old_list']));

    $get_ini = get_config();
    $date_str = explode('-', $list_date);
    $get_dir = "/^playlist_path.*\$/m";
    preg_match_all($get_dir, $get_ini, $matches);
    $line = implode("\n", $matches[0]);
    $path_root = explode("= ", $line)[1];
    $xml_path = $path_root . "/" . $date_str[0] . "/" . $date_str[1];
    $xml_output = $xml_path . "/" . $list_date . ".xml";

    $fill = shell_exec("./sh/fill.sh '".$list_date."' '".$diff_len."' '".$start_time."'");

    // create xml head
    $xml_str = sprintf('<playlist>
    <head>
        <meta name="author" content="example"/>
        <meta name="title" content="Live Stream"/>
        <meta name="copyright" content="(c)%s example.org"/>
        <meta name="date" content="%s"/>
    </head>
    <body>%s', $date_str[0], $list_date, "\n");

    // create xml video element
    foreach($raw_arr as $rawline) {
        $formated_src = str_replace('&', '&amp;', $rawline->src);
        $xml_str .= sprintf('        <video src="%s" begin="%s" dur="%s" in="%s" out="%s"/>%s', $formated_src, $rawline->begin, $rawline->dur, $rawline->in, $rawline->out, "\n");
    }

    // add filled clips
    $xml_str .= $fill;

    // crate xml end
    $xml_str .= "    </body>\n</playlist>\n";

    if (!is_dir($xml_path)) {
        mkdir($xml_path, 0777, true);
    }

    file_put_contents($xml_output, $xml_str);
    printf('Filled and save playlist "%s.xml" done...', $list_date);
}
?>
