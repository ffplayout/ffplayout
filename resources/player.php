<?php

// get config file
function get_config() {
    return file_get_contents("/etc/ffplayout/ffplayout.conf");
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
    $get_ini = get_config();
    $get_dir = "/^playlist_path.*\$/m";
    preg_match_all($get_dir, $get_ini, $match_dir);
    $line = implode("\n", $match_dir[0]);
    $path_root = explode("= ", $line)[1];

    // list start
    $get_start = "/^day_start.*\$/m";
    preg_match_all($get_start, $get_ini, $match_start);
    $start_line = implode("\n", $match_start[0]);
    $start_hour = explode("= ", $start_line)[1];

    $time = date("H");

    if ($time < $start_hour) {
        $date = date("Y-m-d", strtotime( '-1 days' ) );
    } else {
        $date = date("Y-m-d");
    }

    $date_str = explode('-', $date);
    $xml_path = $path_root . "/" . $date_str[0] . "/" . $date_str[1] . "/" . $date . ".xml";

    if (file_exists($xml_path)) {
        $xml = simplexml_load_file($xml_path) or die("Error: Cannot create object");

        $src_re = array();
        $src_re[0] = '/# [0-9-]+.('.$ext.')$/';
        $src_re[1] = '/^[0-9]+ # /';
        $src_re[2] = '/.('.$ext.')$/';
        $src_re[3] = '/^# /';

        $videos = array();

        foreach($xml->body[0]->video as $video) {
            $src       = preg_replace('/^\//', '', $video['src']);
            $src_arr   = explode('/', $src);
            $name      = preg_replace($src_re, '', end($src_arr));
            $name      = str_replace('§', '?', $name);
            $begin     = $video['begin'];
            $dur       = $video['dur'];
            $in        = $video['in'];
            $out       = $video['out'];

            $videos[] = array('start' => $start_hour, 'begin'=> $begin, 'src' => $name, 'dur' => $dur, 'in' => $in, 'out' => $out);
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
