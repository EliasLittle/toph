import { defineConfig } from 'vite';

export default defineConfig({
  server: {
    // Required for SharedArrayBuffer / cross-origin isolation if needed later.
    headers: {
      'Cross-Origin-Opener-Policy': 'same-origin',
      'Cross-Origin-Embedder-Policy': 'require-corp',
    },
  },
});
