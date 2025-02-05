ALTER TABLE configurations ADD processing_override_filter INTEGER NOT NULL DEFAULT 0;
ALTER TABLE channels ADD advanced_id INTEGER REFERENCES advanced_configurations (id) ON UPDATE CASCADE ON DELETE SET DEFAULT;

ALTER TABLE advanced_configurations
DROP filter_pad_scale_w;

ALTER TABLE advanced_configurations
DROP filter_pad_scale_h;

ALTER TABLE advanced_configurations ADD name TEXT;

UPDATE advanced_configurations SET name = 'Default';

INSERT INTO
    advanced_configurations (
        channel_id,
        decoder_input_param,
        decoder_output_param,
        ingest_input_param,
        filter_deinterlace,
        filter_scale,
        filter_overlay_logo_scale,
        filter_overlay_logo,
        name
    )
VALUES
    (
        1,
        '-thread_queue_size 1024 -hwaccel_device 0 -hwaccel cuvid -hwaccel_output_format cuda',
        '-c:v h264_nvenc -preset p2 -tune ll -b:v 50000k -minrate 50000k -maxrate 50000k -bufsize 25000k -c:a s302m -strict -2 -sample_fmt s16 -ar 48000 -ac 2',
        '-thread_queue_size 1024 -hwaccel_device 0 -hwaccel cuvid -hwaccel_output_format cuda',
        'yadif_cuda=0:-1:0',
        'scale_cuda={}:{}:format=yuv420p',
        'null',
        'overlay_cuda={}:shortest=1',
        'Nvidia'
    );

INSERT INTO
    advanced_configurations (
        channel_id,
        decoder_input_param,
        decoder_output_param,
        ingest_input_param,
        filter_deinterlace,
        filter_fps,
        filter_scale,
        filter_overlay_logo_scale,
        filter_overlay_logo,
        name
    )
VALUES
    (
        1,
        '-hwaccel qsv -init_hw_device qsv=hw -filter_hw_device hw -hwaccel_output_format qsv',
        '-c:v mpeg2_qsv -g 1 -b:v 50000k -minrate 50000k -maxrate 50000k -bufsize 25000k -c:a s302m -strict -2 -sample_fmt s16 -ar 48000 -ac 2',
        '-hwaccel qsv -init_hw_device qsv=hw -filter_hw_device hw -hwaccel_output_format qsv',
        'deinterlace_qsv',
        'vpp_qsv=framerate=25',
        'scale_qsv={}:{}',
        'scale_qsv={}',
        'overlay_qsv={}:shortest=1,vpp_qsv=format=nv12',
        'QSV'
    );
