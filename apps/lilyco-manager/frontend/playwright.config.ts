import { defineConfig } from '@playwright/test'
export default defineConfig({ testDir: './tests', timeout: 30000, retries: 0, use: { headless: true }, reporter: [['list']] })
