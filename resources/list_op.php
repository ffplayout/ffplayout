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

// get time_shift
if (!empty($_GET['time_shift'])) {
    $get_ini = get_config();
    $get_shift = "/^time_shift.*\$/m";
    preg_match_all($get_shift, $get_ini, $shift_arr);
    $line_shift = implode("\n", $shift_arr[0]);
    $time_shift = explode("= ", $line_shift)[1];

    if(empty($time_shift)) {
        echo 0;
    } else {
        echo $time_shift;
    }
}

/* -----------------------------------------------------------------------------
json playlist operations
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

// read from json file
// formate the values and generate readeble output
if(!empty($_GET['json_path'])) {
    $json_date = $_GET['json_path'];
    $date_str = explode('-', $json_date);
    $get_ini = get_config();
    $get_dir = "/^playlist_path.*\$/m";
    preg_match_all($get_dir, $get_ini, $matches);
    $line = implode("\n", $matches[0]);
    $path_root = explode("= ", $line)[1];

    $json_path = $path_root . "/" . $date_str[0] . "/" . $date_str[1] . "/" . $json_date . ".json";

    if (file_exists($json_path)) {
        $content = file_get_contents($json_path) or die("Error: Cannot create object");
        $json = json_decode($content, true);

        list($hh, $mm, $ss) = explode(":", $json["begin"]);
        list($l_hh, $l_mm, $l_ss) = explode(":", $json["length"]);

        $start = $hh * 3600 + $mm * 60 + $ss;
        $length = $l_hh * 3600 + $l_mm * 60 + $l_ss;

        $src_re = array();
        $src_re[0] = '/# [0-9-]+.('.$ext.')$/';
        $src_re[1] = '/^[0-9]+ # /';
        $src_re[2] = '/.('.$ext.')$/';
        $src_re[3] = '/^# /';

        foreach($json["program"] as $video) {
            $src            = preg_replace('/^\//', '', $video['source']);
            $src_arr        = explode('/', $src);
            $name           = preg_replace($src_re, '', end($src_arr));
            $name           = str_replace('§', '?', $name);
            $clipBegin      = $start;
            $begin          = gmdate("H:i:s", intval($clipBegin));
            $dur            = $video['duration'];
            $duration       = gmdate("H:i:s", intval($dur));
            $in             = $video['in'];
            $in_p           = gmdate("H:i:s", intval($in));
            $out            = $video['out'];
            $out_p          = gmdate("H:i:s", intval($out));

            $start += $out - $in;

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
    $path = rawurldecode($_GET['li_path']);

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
    $json_path = $path_root . "/" . $date_str[0] . "/" . $date_str[1];
    $json_output = $json_path . "/" . $date . ".json";

    $beginRaw = round($raw_arr[0]->begin);
    $start = sprintf('%02d:%02d:%02d', ($beginRaw/3600),($beginRaw/60%60), $beginRaw%60);

    // prepare header
    $list = array(
        "channel" => "Test 1",
         "date" => $date,
         "begin" => $start,
         "length" => "24:00:00.000",
         "program" => []
    );

    $length = 0;

    // create json video element
    foreach($raw_arr as $rawline) {
        $clipItem = array(
            "in" => floatval($rawline->in),
            "out" => floatval($rawline->out),
            "duration" => floatval($rawline->dur),
            "source" => intval($rawline->src)
        );

        $list["program"][] = $clipItem;

        $length += round($rawline->out - $rawline->in);
    }

    $list["program"]["length"] = sprintf('%02d:%02d:%02d', ($length/3600),($length/60%60), $length%60);

    if (!is_dir($json_path)) {
        mkdir($json_path, 0777, true);
    }

    file_put_contents($json_output, json_encode(
        $list, JSON_UNESCAPED_UNICODE|JSON_UNESCAPED_SLASHES|JSON_PRETTY_PRINT));
    printf('Save playlist "%s.json" done...', $date);
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
    $json_path = $path_root . "/" . $date_str[0] . "/" . $date_str[1];
    $json_output = $json_path . "/" . $list_date . ".json";

    $fill = shell_exec("./sh/fill.sh '".$diff_len."'");

    $beginRaw = round($raw_arr[0]->begin);
    $start = sprintf('%02d:%02d:%02d', ($beginRaw/3600),($beginRaw/60%60), $beginRaw%60);

    // prepare header
    $list = array(
        "channel" => "Test 1",
         "date" => $list_date,
         "begin" => $start,
         "length" => "24:00:00.000",
         "program" => []
    );

    // create json video element
    foreach($raw_arr as $rawline) {
        $clipItem = array(
            "in" => floatval($rawline->in),
            "out" => floatval($rawline->out),
            "duration" => floatval($rawline->dur),
            "source" => $rawline->src
        );

        $list["program"][] = $clipItem;
    }

    foreach(preg_split("/((\r?\n)|(\r\n?))/", $fill) as $line){
        $line_arr = explode('|', $line);

        $clipItem = array(
            "in" => floatval($line_arr[0]),
            "out" => floatval($line_arr[1]),
            "duration" => floatval($line_arr[2]),
            "source" => $line_arr[3]
        );

        if ($line_arr[3]) {
            $list["program"][] = $clipItem;
        }
    }

    if (!is_dir($json_path)) {
        mkdir($json_path, 0777, true);
    }

    file_put_contents($json_output, json_encode(
        $list, JSON_UNESCAPED_UNICODE|JSON_UNESCAPED_SLASHES|JSON_PRETTY_PRINT));
    printf('Filled and save playlist "%s.json" done...', $list_date);
}
?>
