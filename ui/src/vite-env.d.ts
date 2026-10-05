/// <reference types="vite/client" />

interface ImportMetaEnv {
  /** "mock" selects the in-memory backend (`npm run dev:mock`). */
  readonly VITE_BACKEND?: string;
}

interface ImportMeta {
  readonly env: ImportMetaEnv;
}
