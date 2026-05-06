import { defineConfig } from '@playwright/test';

export default defineConfig({
  testDir: './tests/browser',
  timeout: 15_000,
  use: {
    baseURL: 'http://127.0.0.1:8989',
    headless: true,
  },
  // M4: 4 workers is stabiel zonder thermische throttle
  workers: 4,
  reporter: [['list'], ['json', { outputFile: 'docs/intelligence/playwright-results.json' }]],
});
