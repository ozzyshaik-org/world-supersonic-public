import { defineConfig } from 'vite';

// base './' keeps dist/ portable (sub-path hosting and file:// inspection)
export default defineConfig({
  base: './',
  build: {
    target: 'es2022',
    outDir: 'dist',
    rollupOptions: {
      output: {
        manualChunks(id) {
          if (id.includes('node_modules/three/')) return 'three';
          if (id.includes('node_modules/gsap/')) return 'motion';
        },
      },
    },
  },
});
