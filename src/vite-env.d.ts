/// <reference types="vite/client" />

// `tsconfig.json` declares no `types` entry, so nothing pulls Vite's ambient
// client types in on its own. Without them the `?raw` suffix on an import — the
// only way the file-type icons reach the bundle as inline SVG strings — has no
// module declaration and fails to typecheck. This file exists solely to bring
// them in; it deliberately declares nothing of its own.
