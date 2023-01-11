export {}

declare global {
    interface Crumb {
        text: string
        path: string
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

    interface FolderObject {
        source: string
        parent: string
        folders: string[]
        files: FileObject[]
    }

    interface SourceObject {
        type: string
        src: string
    }
}
