<?php
error_reporting(E_ALL);
ini_set('display_errors', 'On');

// get playout log
if(!empty($_GET['log_from'])) {
    $log_from = $_GET['log_from'];

    if ($log_from === "playing") {
        $get_ini = file_get_contents("/etc/ffplayout/ffplayout.conf");
        $get_path = "/^log_file.*\$/m";
        preg_match_all($get_path, $get_ini, $matches);
        $line = implode("\n", $matches[0]);
        $log_file = explode("= ", $line)[1];

        $open_log = fopen($log_file, "r") or die("Unable to open file!");
        echo fread($open_log,filesize($log_file));
        fclose($open_log);
    }

    if ($log_from === "system") {
        echo shell_exec("sudo /bin/journalctl -u ffplayout.service -n 1000");
    }
}

?>
