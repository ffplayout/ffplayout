import type { JwtPayload } from 'jwt-decode'
import type { PlayoutConfig, Playlist as Ply } from './playout_config'

export {}

declare global {
    interface JwtPayloadExt extends JwtPayload {
        id: number
        channels: number[]
        role: string
    }

    interface PlaylistExt extends Ply {
        startInSec: number,
        lengthInSec: number
    }

    interface PlayoutConfigExt extends PlayoutConfig {
        playlist: PlaylistExt
    }

    interface PlayoutOutput {
        id: number
        name: string
        hls_variants: string
        stream_url: string
        hls_playlist_name: string | null
        hls_segment_duration: number | null
        hls_list_size: number | null
        desktop_fullscreen: boolean
        width: number
        height: number
        fps: number
        video_preset: string | null
        rate_control: string | null
        video_quality: number | null
        video_maxrate: number | null
        audio_bitrate: number | null
        channel_id: number
    }

    interface TextPreset {
        id: number
        channel_id: number
        name: string
        text: string
        use_filename: boolean
        font_family: string
        font_weight: 'normal' | 'semibold' | 'bold'
        filename_regex: string
        position_x: string
        position_y: string
        font_size: number
        line_spacing: number
        text_color: string
        text_opacity: number
        background_enabled: boolean
        background_color: string
        background_opacity: number
        background_padding: number
        opacity: number
        scroll_direction: 'none' | 'left_to_right' | 'right_to_left'
        scroll_speed: number
        scroll_repeat: number
        fade_in_seconds: number
        fade_out_seconds: number
    }

    interface Token {
        access: string
        refresh: string
    }

    interface DataAuth {
        uuid: string
    }

    interface Channel {
        id: number
        extra_extensions: string | string[]
        name: string
        preview_url: string
        public: string
        playlists: string
        storage: string
        timezone?: string
    }

    interface User {
        id: number
        username: string
        mail?: string
        password?: string
        confirm?: string
        admin?: boolean
        channel_ids?: number[]
        role_id?: number
        two_factor: boolean
    }

    interface Crumb {
        text: string
        path: string
    }

    interface Payload {
        method: string
        headers: any
        body?: any
    }

    interface Playlist {
        channel: string
        date: string
        program: PlaylistItem[]
    }

    interface PlaylistItem {
        date?: string
        uid: string
        begin: number
        title?: string | null
        source: string
        duration: number
        in: number
        out: number
        category?: string
        overtime?: boolean
    }

    interface FileObject {
        name: string
        duration: number
    }

    interface Folder {
        uid: string
        name: string
    }

    interface FileFolderObject {
        source: string
        parent: string
        parent_folders: Folder[]
        folders: Folder[]
        files: FileObject[]
    }

    interface FolderObject {
        source: string
        parent: string
        folders: Folder[]
    }

    interface SourceObject {
        type: string
        src: string
    }

    interface TemplateItem {
        start: string
        duration: string
        shuffle: boolean
        paths: string[] | any[]
    }

    interface Template {
        sources: TemplateItem[]
    }

    interface BodyObject {
        paths?: string[]
        template?: Template
    }

    interface SystemStatistics {
        cpu: { cores: number; usage: number }
        load: { one: number; five: number; fifteen: number }
        memory: { total: number; used: number; free: number }
        network?: { name: string; current_in: number; current_out: number; total_in: number; total_out: number }
        storage?: { path: string; total: number; free: number }
        swap: { total: number; used: number; free: number }
        system: { name?: string; kernel?: string; version?: string; ffp_version?: string }
    }

    interface PlayoutStatus {
        media: PlaylistItem
        index: number
        audio?: AudioLevel
        ingest: boolean
        mode: string
        elapsed: number
        shift: number
        title?: string
    }

    interface AudioLevel {
        rms_db: number
        peak_db: number
    }

    interface SplitTime {
        id: number
        val: number
    }

    declare namespace Intl {
        type Key = "calendar" | "collation" | "currency" | "numberingSystem" | "timeZone" | "unit";

        function supportedValuesOf(input: Key): string[];
      }
}
