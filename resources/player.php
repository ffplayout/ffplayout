<?php

// get config file
function get_ini() {
    return parse_ini_file("/etc/ffplayout/ffplayout.conf", TRUE, INI_SCANNER_RAW);
}

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

// get current track
if(!empty($_GET['track'])) {
    $ini = get_ini();
    $dir = $ini['PLAYLIST']['path'];

    // list start
    $start = $ini['PLAYLIST']['day_start'];
    $st = date_parse($start);
    $start_time = $st['hour'] * 3600 + $st['minute'] * 60 + $st['second'];

    $t = date_parse(date("H:i:s"));
    $time = $t['hour'] * 3600 + $t['minute'] * 60 + $t['second'];

    if ($time < $start_time) {
        $date = date("Y-m-d", strtotime( '-1 days' ) );
    } else {
        $date = date("Y-m-d");
    }

    $date_str = explode('-', $date);
    $json_path = $dir . "/" . $date_str[0] . "/" . $date_str[1] . "/" . $date . ".json";

    if (file_exists($json_path)) {
        $content = file_get_contents($json_path) or die("Error: Cannot create object");
        $json = json_decode($content, true);

        list($hh, $mm, $ss) = explode(":", $json["begin"]);
        list($l_hh, $l_mm, $l_ss) = explode(":", $json["length"]);
        $begin = $hh * 3600 + $mm * 60 + $ss;
        $length = $l_hh * 3600 + $l_mm * 60 + $l_ss;

        $src_re = array();
        $src_re[0] = '/# [0-9-]+.('.$ext.')$/';
        $src_re[1] = '/^[0-9]+ # /';
        $src_re[2] = '/.('.$ext.')$/';
        $src_re[3] = '/^# /';

        $videos = array();

        foreach($json["program"] as $video) {
            $src       = preg_replace('/^\//', '', $video['source']);
            $src_arr   = explode('/', $src);
            $name      = preg_replace($src_re, '', end($src_arr));
            $name      = str_replace('§', '?', $name);
            $dur       = $video['duration'];

            $in        = $video['in'];
            $out       = $video['out'];

            $videos[] = array('start' => $start_time, 'begin'=> $begin, 'src' => $name, 'dur' => $dur, 'in' => $in, 'out' => $out);

            $begin += $out - $in;
        }

        echo json_encode($videos);
    }
}

// start / stop playout
if(!empty($_POST['playout'])) {
    $state = $_POST['playout'];

    if ($state === "start") {
        $out = shell_exec("./sh/playout.sh start");
        echo "<b>Started Playout</b>";
    } else if ($state === "stop") {
        $out = shell_exec("./sh/playout.sh stop");
        echo "<b>Stoped Playout</b>";
    }

    echo "$out";
}
