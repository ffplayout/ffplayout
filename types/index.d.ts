export { }

declare global {
    interface GuiConfig {
        id: number
        config_path: string
        extra_extensions: string | string[]
        name: string
        preview_url: string
        service: string
        uts_offset?: number
    }

    interface User {
        username: String
        mail: String
        password?: String
        confirm?: String
        role_id?: Number
    }

   interface Crumb {
        text: string
        path: string
    }

    interface Payload {
        method: string,
        headers: any,
        body?: any,
    }

    interface PlaylistItem {
        uid: string
        begin: number
        source: string
        duration: number
        in: number
        out: number
        audio?: string
        category?: string
        custom_filter?: string
        class?: string
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
}
