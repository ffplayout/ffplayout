import { timezone } from 'dayjs/plugin/timezone.js';
import type { JwtPayload } from 'jwt-decode'
import type { AdvancedConfig } from './advanced_config'
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
        parameters: string
        channel_id: number
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
        audio?: string
        category?: string
        custom_filter?: string
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
        paths: string[]
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
        storage?: { path: string; total: number; used: number }
        swap: { total: number; used: number; free: number }
        system: { name?: string; kernel?: string; version?: string; ffp_version?: string }
    }

    interface PlayoutStatus {
        media: PlaylistItem
        index: number
        ingest: boolean
        mode: string
        elapsed: number
        shift: number
        title?: string
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
