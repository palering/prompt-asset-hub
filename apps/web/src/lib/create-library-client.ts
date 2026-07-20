import type { LibraryClient } from "./library-client";
import { MockLibraryClient } from "./mock-library-client";
import { TauriLibraryClient } from "./tauri-library-client";

export function isTauriRuntime(): boolean {
  return typeof window !== "undefined" && Boolean(window.__TAURI_INTERNALS__);
}

export function createLibraryClient(): LibraryClient {
  return isTauriRuntime() ? new TauriLibraryClient() : new MockLibraryClient();
}
