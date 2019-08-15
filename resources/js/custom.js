/* -----------------------------------------------------------------------------
global functions
------------------------------------------------------------------------------*/

// modal function:
// display an overlay window for warings, clip previews, etc.
function modal(btOK, btCan, video, title, content, x, y, callback) {
    if (video) {
        text = '<video id="preview_player" class="video-js" controls preload="auto" autoplay="true" data-setup={}> <source src="' + content + '" type="video/mp4" /> </video>';
    } else {
        text = content;
    }

    var modalStr = '<p>' + text + '</p>';

    $('#dialog-confirm').html(modalStr);

    $("#dialog-confirm").dialog({
        title: title,
        resizable: false,
        height: y,
        width: x,
        modal: true,
        buttons: [{
                id: "button-ok",
                text: "Ok",

                click: function() {
                    $('#preview_player').remove()
                    $(this).dialog("close");
                    callback(true);
                }
            },
            {
                id: "button-cancel",
                text: "Cancel",

                click: function() {
                    $(this).dialog("close");
                    callback(false);
                }
            }
        ]
    });

    if (!btOK) {
        $("#button-ok").remove();
    }
    if (!btCan) {
        $("#button-cancel").remove();
    }
}

//get date
function get_date(t, seek_day) {
    var datetime = moment();
    var l_time = t.split(":");
    l_stamp = parseInt(l_time[0]) * 3600 + parseInt(l_time[1]) * 60 + parseInt(l_time[2]);
    t_stamp = moment.duration(datetime.format("H:M:S")).asSeconds();

    if (l_stamp > 0 && l_stamp > t_stamp && seek_day) {
        return datetime.add(-1, 'days');
    } else {
        return datetime;
    }
}

// calendar settings
$('.calender').pignoseCalendar({
    lang: 'de',
    theme: 'dark',
    date: moment(),
    format: 'YYYY-MM-DD',
    week: 0,
    init: function() {
        $(this).find('.pignose-calendar-unit-active').children().addClass('calender-bold');
    },
    select: function(date) {
        $.get("resources/list_op.php?list_start=get", function(result) {
            var list_date = get_date(result, true);

            if (date[0].format('YYYY-MM-DD') === list_date.format('YYYY-MM-DD')) {
                $('#playlistBody').attr('listday', list_date.format('YYYY-MM-DD'));
                get_json(list_date.format('YYYY-MM-DD'), true);
            } else {
                $('#playlistBody').attr('listday', date[0].format('YYYY-MM-DD'));
                get_json(date[0].format('YYYY-MM-DD'), true);
            }
        });
    }
});

// make directory listen resizeble
$('#mainNav').resizable({
    handles: 'e, w'
});

/* -----------------------------------------------------------------------------
directory listen functions
------------------------------------------------------------------------------*/

// initalize file browser click functions
// this is nessesary because the folder list gets generated from php
// and javascript don't know them before the init time
function init_browse_click() {
    // folder navigation
    // all clickable links (folders and files) has the clearfix class
    $('.clearfix').click(function(e) {
        var current_data = $(this).attr("href");
        browse(current_data);

        // level_up navigate to the parent directory
        if ($(this).find("i").hasClass('fa-level-up')) {
            var level_up = current_data.split("?").pop();
            if (!level_up.match("http")) {
                browse('?' + level_up);
            }
        }
        e.preventDefault();
    });

    // header navigation
    $('.breadcumb').click(function(e) {
        var current_data = $(this).attr("href");
        var change_level = current_data.split("?").pop();

        browse('?' + change_level);
        e.preventDefault();
    });

    // video preview
    // when click on file link, open the modal windows and preview the clip
    $('.file-info-button').click(function(e) {
        var current_element = $(this);
        var rawpath = current_element.closest('li').attr('data-href').replace(/^\?dir=/g, current_element);

        modal(true, null, true, decodeURIComponent(rawpath.split("/").pop()), rawpath, 1039, 716, function(result) {});

        e.preventDefault();
    });
}

// call php script to navigatge to the selected folder on the server
function browse(c_data) {
    $.get("functions.php" + c_data + "&head=get", function(data) {
        $('#browserHead').html(data);
    });

    $.get("functions.php" + c_data + "&ul=get", function(data) {
        $('#rootDirectory').html(data);
        init_browse_click();
    });
}

// first page load will open the media directory
window.onload = function() {
    $.get("resources/list_op.php?clips_root=get", function(result) {
        browse("?dir=" + result.replace(/^\//g, ''));
    });

    $.get("resources/list_op.php?list_start=get", function(result) {
        var l_time = result.split(":");
        var l_stamp = parseInt(l_time[0]) * 3600 + parseInt(l_time[1]) * 60 + parseInt(l_time[2]);

        var list_date = get_date(result, true);
        // write playlist date to list attribute, for later use
        $('#playlistBody').attr('listday', list_date.format("YYYY-MM-DD"));
        $('#playlistBody').attr('liststart', l_stamp);
        // read playlist from current day
        get_json(list_date.format("YYYY-MM-DD"), true);
    });
}

/* -----------------------------------------------------------------------------
playlist functions
------------------------------------------------------------------------------*/

// initalize buttons and input dialogs,
// after the playlist has loaded
function init_list_op() {
    // video preview
    $('.file-play').click(function(e) {
        var file_path = $(this).attr('data-href');
        var play_URL = encodeURIComponent(file_path);

        modal(true, null, true, decodeURIComponent(file_path.split("/").pop()), play_URL, 1039, 716, function(result) {});

        e.preventDefault();
    });

    var enableButton = function(){
        $('.row-del').removeClass('disabled');
    }

    // delete clip from playlist
    $('.row-del').click(function() {
        if (!$(this).hasClass('disabled')) {
            $(this).parent().parent().remove();
            reorder_playlist();
            $('.row-del').addClass('disabled');
            // limit click rate
            setTimeout(enableButton, 1000);
            return true;
        } else {
            modal(true, null, null, "Delete Item", "Removing items is limited to every second.", 'auto', 'auto', function(result) {});
        }
    });

    // input field for seek in clip
    $('.input-in').change(function(){
        var in_seconds = moment.duration($(this).val()).asSeconds();
        var in_duration = $(this).closest('ul').parent().attr('dur');
        if (in_seconds > in_duration) {
            modal(true, null, null, "Seek in Video", "Seek Value is bigger then Duration!<br/>Please fix that...", 'auto', 'auto', function(result) {});
            $(this).val("00:00:00");
        } else {
            $(this).closest('ul').parent().attr('in', in_seconds);
            reorder_playlist();
        }
    });

    // input field for cut out clip at specific time
    $('.input-out').change(function(){
        var cur_val = $(this).val();
        var out_seconds = moment.duration($(this).val()).asSeconds();
        var out_dur = $(this).closest('ul').parent().attr('dur');
        if (out_seconds > out_dur) {
            modal(true, null, null, "Cut Video", "Cut Value is bigger then Duration!<br/>Please fix that...", 'auto', 'auto', function(result) {});
            $(this).val(cur_val);
        } else {
            $(this).closest('ul').parent().attr('out', out_seconds);
            reorder_playlist();
        }
    });

    // reset button
    $('#bt_reset').click(function() {
        // scroll to playlist top
        get_json($('#playlistBody').attr('listday'), false);
    });
}

// read formated playlist from php function
function get_json(date, jump) {
    $.get("resources/list_op.php?json_path=" + date, function(result) {
        $('#playlistBody').html(result);
        init_list_op();
        if (jump) {
            jump_and_colorize_title(true);
        } else {
            jump_and_colorize_title(false);
        }
    });
}

// when new clips are dragged,
// or clips are moved, or deleted, calculate the starting time new
// it also take care of the in and out time
function reorder_playlist() {
    // get start time from: /etc/ffplayout/ffplayout.conf
    $.get("resources/list_op.php?list_start=get", function(result) {
        var reorder = document.getElementById("playlistBody").getElementsByClassName("list-item");
        var l_time = result.split(":");
        var start_time = parseFloat(l_time[0]) * 3600 + parseFloat(l_time[1]) * 60 + parseFloat(l_time[2]);
        var time_format, cur_in, cur_out;

        for (var i = 0; i < reorder.length; i++) {
            time_format = moment.utc(start_time * 1000).format('HH:mm:ss');
            cur_in = parseFloat($(reorder[i]).attr('in'));
            cur_out = parseFloat($(reorder[i]).attr('out'));
            $(reorder[i]).attr('begin', start_time);
            $(reorder[i]).removeClass('current_item next_item last_items');
            $(reorder[i]).find('.row-start').html(time_format);
            start_time += cur_out - cur_in;
        }

        init_list_op();
        jump_and_colorize_title(false);
    });
}


// jump to right time in playlist
// and colorize the list
function jump_and_colorize_title(jump) {
    var moment_time = moment().format('HH:mm:ss');
    var time_in_seconds = parseFloat(moment.duration(moment_time).asSeconds());

    var play_items = document.getElementById("playlistBody").getElementsByClassName("list-item");
    var play_begin, play_dur;

    $.get("resources/list_op.php?list_start=get", function(result) {
        var list_date = get_date(result, true);

        if (list_date.format("H") < parseInt(result)) {
            time_in_seconds += 86400
        }

        if ($('#playlistBody').attr('listday') === list_date.format("YYYY-MM-DD")) {
            for (var i = 0; i < play_items.length; i++) {
                play_begin = parseFloat($(play_items[i]).attr('begin'));
                play_dur = parseFloat($(play_items[i]).attr('dur'));
                if (play_begin + play_dur >= time_in_seconds) {
                    // jump to position only after page load
                    if (jump) {
                        $('#list-container').animate({
                            scrollTop: $('#playlistBody li:nth-child(' + (i-1) + ')').position().top
                        }, 500, "easeOutQuint");
                    }

                    // colorize items
                    $(play_items[i]).addClass('current_item');
                    $(play_items[i+1]).addClass('next_item');
                    $('.list-item:gt('+(i+1)+')').addClass('last_items');
                    break;
                }
            }
        } else {
            // scroll to playlist top
            if (jump) {
                $('#list-container').animate({scrollTop: 0}, 500, "easeOutQuint");
            }
        }
    });
}


/* -----------------------------------------------------------------------------
footer functions
------------------------------------------------------------------------------*/

function get_log(logtype) {
    $.get("resources/logging.php?log_from=" + logtype, function(result) {
        $('#output').html(result);
        scroll_div = $('.log-content div');

        scroll_div.animate({
            scrollTop: scroll_div[0].scrollHeight - scroll_div[0].clientHeight
        }, 500, "easeOutQuint");
    });
}

// logging tabs
$('#bt_play').click(function() {
    $(this).addClass("active");
    $('#bt_sys').removeClass("active");

    get_log("playing");
});

$('#bt_sys').click(function() {
    $(this).addClass("active");
    $('#bt_play').removeClass("active");

    get_log("system");
});


/* -----------------------------------------------------------------------------
header functions
------------------------------------------------------------------------------*/
var intervalId = null;

function get_track_list(interval) {
    var begin, seek, out, time_left;

    $.get("resources/player.php?track=get", function(result) {
        function get_track() {
            var moment_time = moment().format('HH:mm:ss');
            var time_in_seconds = parseFloat(moment.duration(moment_time).asSeconds());
            var json = $.parseJSON(result);
            var playlist_start = parseFloat($('#playlistBody').attr('liststart'));

            if (0.0 <= time_in_seconds && time_in_seconds < playlist_start) {
                time_in_seconds += 86400.0;
            }

            begin = playlist_start;

            $.each(json, function (_index, value) {
                seek = parseFloat(value['in']);
                out = parseFloat(value['out']);

                if (time_in_seconds < begin + out - seek ) {
                    time_left = begin + out - seek - time_in_seconds;
                    $('#countdown').html(moment.utc(time_left * 1000).format('HH:mm:ss'));
                    $('#title').html((value['src']));
                    return false;
                }

                begin += out - seek;
            });
        }
        if (interval) {
            refreshIntervalId = setInterval(get_track, 1000);
        } else {
            clearInterval(refreshIntervalId);
        }
    });
}

function set_time() {
    $('#clock').html(moment().format('H:mm:ss'));
}

// start stream
$('#bt_start').click(function() {
    $.ajax({
        url: "resources/player.php",
        type: "POST",
        data: "playout=start",
        beforeSend: function() {
            modal(false, null, null, "Start Playout", '<div style="text-align:center; min-width: 120px"><img src="resources/img/35.png" height="46" width="46"></div>', 'auto', 'auto', function(result) {});
        },
        success: function(result) {
            modal(true, null, null, "Start Playout", '<div style="text-align:center;min-width: 120px">' + result + '</div>', 'auto', 'auto', function(result) {});

            videojs('myStream').play();
            get_track_list(true);
            get_log("playing");
        },
    });
});

// stop stream
$('#bt_stop').click(function() {
    modal(true, true, null, "Stop Playout", '<div style="text-align:center;min-width: 120px">Are you really sure, you want to do this?</div>', 'auto', 'auto', function(result) {
        if (result) {
            $.ajax({
                url: "resources/player.php",
                type: "POST",
                data: "playout=stop",
                beforeSend: function() {
                    modal(false, null, null, "Stop Playout", '<div style="text-align:center; min-width: 120px"><img src="resources/img/35.png" height="46" width="46"></div>', 'auto', 'auto', function(result) {});
                },
                success: function(result) {
                    modal(true, null, null, "Stop Playout", '<div style="text-align:center;min-width: 120px">' + result + '</div>', 'auto', 'auto', function(result) {});

                    videojs('myStream').pause();
                    get_track_list(false);
                },
            });
        }
    });
});

/* -----------------------------------------------------------------------------
ready state
------------------------------------------------------------------------------*/

// when page is loaded, do something with the staff
$(document).ready(function() {
    // init browser and playlist controlls
    init_browse_click();
    init_list_op();
    get_log("playing");

    // set player standards
    videojs('myStream', {techOrder: ['flash', 'html5']});
    videojs('myStream').volume(0.2);

    setInterval(set_time, 1000);
    get_track_list(true);
    /*-------------------------------------------
    sorting
    --------------------------------------------*/

    // sort: drag and drop function from directory listen to playlist
    var el = document.getElementById('rootDirectory');
    Sortable.create(el, {
        animation: 200,
        filter: ".folder",
        group: {
            name: "shared",
            pull: "clone",
            revertClone: true,
        },
        sort: false,
        onStart: function(evt) {
            $(evt.item).addClass('list-item').removeClass('file');
        },
        onEnd: function(evt) {
            var itemEl = evt.item;
            if ($(itemEl).parent().attr('class') === "list-group") {
                var url = $(itemEl).find("a").attr("href");
                var lis = document.getElementById("playlistBody").getElementsByClassName("list-item");

                for (li of lis) {
                    if ($(li).attr('src') === "None") {
                        $(li).remove();
                    }
                }

                $.get("resources/list_op.php?li_path=" + "/" + url, function(result) {
                    $(itemEl).replaceWith(result);
                    reorder_playlist();
                    init_list_op();
                });
            } else {
                $(evt.item).addClass('file').removeClass('list-item');
            }
        },
    });

    // drag and drop within the playlist
    Sortable.create(playlistBody, {
        group: "shared",
        handle: ".handle",
        ghostClass: 'ghost',
        sort: true,
        onEnd: function(evt) {
            reorder_playlist();
        }
    });

    // save button
    $('#bt_save').click(function() {
        var start_time = parseFloat($('.list-item').first().attr('begin'));
        var last_start = parseFloat($('.list-item').last().attr('begin'));
        var last_in = parseFloat($('.list-item').last().attr('in'));
        var last_out = parseFloat($('.list-item').last().attr('out'));
        var over_length = last_start - start_time + last_out - last_in - 86400;

        if (over_length > 0) {
            modal(true, null, null, "Save Playlist", "Playtime from Playlist is to long!<br/><b>Difference:</b> " + over_length, 'auto', 'auto', function(result) {});
        } else if (over_length < -6) {
            modal(true, null, null, "Save Playlist", "Playtime from Playlist is to short!<br/><b>Difference:</b> " + over_length, 'auto', 'auto', function(result) {});
        } else {
            var save_list = [];
            $('#playlistBody li.list-item').each(function(){
                save_list.push({
                    src:$(this).attr('src'),
                    dur:$(this).attr('dur'),
                    in:$(this).attr('in'),
                    out:$(this).attr('out')
                });
            });

            var json = encodeURIComponent(JSON.stringify(save_list));
            var date = $('#playlistBody').attr('listday');

            $.ajax({
               type: "POST",
               url: "resources/list_op.php",
               data: "date=" + date + "&save=" + json,
               success: function(result) {
                   modal(true, null, null, "Save Playlist", result, 'auto', 'auto', function(result) {});
               }
           });
        }
    });

    // fill end of playlist to get full 24 hours
    $('#bt_filler').click(function() {
        var start_time = parseFloat($('.list-item').first().attr('begin'));
        var last_start = parseFloat($('.list-item').last().attr('begin'));
        var last_in = parseFloat($('.list-item').last().attr('in'));
        var last_out = parseFloat($('.list-item').last().attr('out'));
        var missed_length = last_start - start_time + last_out - last_in - 86400;

        if (missed_length > 0) {
            modal(true, null, null, "Fill Playlist", "Playtime from Playlist is to long!<br/><b>Difference:</b> " + missed_length, 'auto', 'auto', function(result) {});
        } else if (missed_length > -6) {
            modal(true, null, null, "Fill Playlist", "Playtime from Playlist is in range!<br/><b>No change will made...</b>", 'auto', 'auto', function(result) {});
        } else if (missed_length < -2700) {
            modal(true, null, null, "Fill Playlist", "Missed length to fill is bigger then 45 minutes!<br/><b>Please add more clips...</b>", 'auto', 'auto', function(result) {});
        } else {
            date = $('#playlistBody').attr('listday');
            var save_list = [];
            $('#playlistBody li.list-item').each(function(){
                save_list.push({
                    src:$(this).attr('src'),
                    dur:$(this).attr('dur'),
                    in:$(this).attr('in'),
                    out:$(this).attr('out')
                });
            });

            var json = encodeURIComponent(JSON.stringify(save_list));
            var date = $('#playlistBody').attr('listday');

            $.ajax({
               type: "POST",
               url: "resources/list_op.php",
               data: "fill_playlist=" + date + "&diff_len=" + Math.abs(missed_length) + "&start_time=" + (last_start + last_out - last_in) + "&old_list=" + json,
               beforeSend: function() {
                   modal(null, null, null, "Fill Playlist", "Filling Playlist in progress...", 'auto', 'auto', function(result) {});
               },
               success: function(result) {
                  $('#dialog-confirm').dialog("close");
                  modal(true, null, null, "Fill Playlist", result + "<br/><b>Filled Time:</b> " + moment.utc(Math.abs(missed_length) * 1000).format('HH:mm:ss'), 'auto', 'auto', function(result) {});
                  get_json(date, false);
               }
           });
        }
    });

});
