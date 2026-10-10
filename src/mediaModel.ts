// These IPC field names remain compatible with the existing native catalog.
export type FileEntry = { name: string; path: string; isDirectory: boolean; size: number | null; modifiedAt: number | null; createdAt?: number | null; videoId?: number | null; rating?: number | null }
export type DirectoryListing = { path: string; parent: string | null; entries: FileEntry[]; catalogWarning?: string }
export type TrashBatchResult = { listing: DirectoryListing; movedCount: number; error: string | null; remainingPaths?: string[] }
