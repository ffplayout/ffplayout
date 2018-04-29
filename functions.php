<?php

// Include the DirectoryLister class
require_once('resources/DirectoryLister.php');

// Initialize the DirectoryLister object
$lister = new DirectoryLister();

// Initialize the directory array
if (isset($_GET['dir'])) {
    $dirArray = $lister->listDirectory($_GET['dir']);
} else {
    $dirArray = $lister->listDirectory('.');
}
?>

<?php
if(!empty($_GET['head'])) {
    $breadcrumbs = $lister->listBreadcrumbs(); ?>
        <p class="navbar-text">
            <?php foreach ($breadcrumbs as $breadcrumb): ?>
                <?php if ($breadcrumb != end($breadcrumbs)): ?>
                    <a href="<?php echo $breadcrumb['link']; ?>" class="breadcumb"><?php echo $breadcrumb['text']; ?></a>
                    <span class="divider">/</span>
                <?php else: ?>
                    <?php echo $breadcrumb['text']; ?>
                <?php endif; ?>
            <?php endforeach; ?>
        </p>
    <div id="directory-list-header">
        <div class="row">
        </div>
    </div>
<?php } ?>

<?php
if(!empty($_GET['ul'])) {
    $get_type = $_GET['dir'];

    foreach($dirArray as $name => $fileInfo):
        if ($get_type === "ADtvMedia" and $name === "..") {
            continue;
        }
        if($fileInfo['icon_class'] === "fa-folder") {
            $type = "folder";
        } else if($fileInfo['icon_class'] === "fa-level-up") {
            $type = "level";
        } else {
            $type = "file";

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

            // formating filenames for the browser
            if (preg_match('/^02 #/', $name) === 1) {
                $name_exp = explode('#' , $name);
                $name_end = preg_split("/[-.]/", $name_exp[4]);
                $in_sec   = $name_end[0] * 60 + $name_end[1];
                $len      = gmdate("H:i:s", $in_sec);
                $name     = $len . ' | ' . $name_exp[2] . '|' . $name_exp[3];
            } elseif (preg_match_all('/#/', $name) === 2) {
                $name_exp = explode('#' , $name);
                $name_end = preg_split("/[-.]/", $name_exp[2]);
                $in_sec   = $name_end[0] * 60 + $name_end[1];
                $len      = gmdate("H:i:s", $in_sec);
                $name     = $len . ' | ' . $name_exp[0] . '|' . $name_exp[1];
            } elseif (preg_match('/# [0-9-]+.('.$ext.')$/', $name) === 1) {
                $name_exp = explode('#' , $name);
                $name_end = preg_split("/[-.]/", end($name_exp));
                $in_sec   = $name_end[0] * 60 + $name_end[1];
                $len      = gmdate("H:i:s", $in_sec);
                $name_pre = preg_replace('/# [0-9-]+.('.$ext.')$/', '', $name);
                $name     = $len . ' | ' . $name_pre;
            }
            $name = str_replace('§', '?', $name);
        }?>
        <li data-name="<?php echo $name; ?>" class="<?php echo $type ." ". $get_type ?>" data-href="<?php echo $fileInfo['url_path']; ?>">
            <a href="<?php echo $fileInfo['url_path']; ?>" class="clearfix" data-name="<?php echo $name; ?>">
                <div class="row">
                    <span class="file-name">
                        <i class="fa <?php echo $fileInfo['icon_class']; ?> fa-fw"></i>
                        <?php echo $name; ?>
                    </span>
                </div>
            </a>
            <?php if (is_file($fileInfo['file_path'])): ?>
                <a href="javascript:void(0)" class="file-info-button">
                    <i class="fa fa-play-circle"></i>
                </a>
            <?php else: ?>
                <?php if ($lister->containsIndex($fileInfo['file_path'])): ?>
                    <a href="<?php echo $fileInfo['file_path']; ?>" class="web-link-button" <?php if($lister->externalLinksNewWindow()): ?>target="_blank"<?php endif; ?>>
                        <i class="fa fa-external-link"></i>
                    </a>
                <?php endif; ?>
            <?php endif; ?>
        </li>
    <?php endforeach;
}
?>
