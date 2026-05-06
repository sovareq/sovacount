import { test, expect } from '@playwright/test';
import * as path from 'path';

// Veronderstelt een draaiende governor-http op :8989 met GOVERNOR_PROVIDER=mock
const BASE = 'http://127.0.0.1:8989';
const SCREENSHOT_DIR = path.resolve(__dirname, '../../docs/intelligence');

test('dashboard laadt en health-pill is groen', async ({ page }) => {
  await page.goto(BASE);
  await expect(page).toHaveTitle(/SovaCount/);
  const pill = page.locator('#health');
  await expect(pill).toHaveClass(/health-ok/);
  await expect(pill).toContainText('v');
});

test('shift ▲ klik activeert knop en schrijft naar server', async ({ page }) => {
  await page.goto(BASE);
  const btn = page.locator('.shift-btn[data-shift="1"]');
  await btn.click();
  await expect(btn).toHaveClass(/active/);
  // Reset
  await page.locator('.shift-btn[data-shift="0"]').click();
});

test('classify-panel: happy path met mock provider', async ({ page }) => {
  await page.goto(BASE);
  await page.locator('#classify-panel textarea').fill(
    'Refactor auth middleware to support OAuth2 scopes'
  );
  await page.locator('#classify-panel button').click();
  // Wacht op resultaat (max 8s — mock is snel)
  await expect(page.locator('#classify-result')).not.toHaveAttribute('hidden', '', { timeout: 8000 });
  // Tier-badge moet tekst bevatten
  const tier = page.locator('#res-tier');
  await expect(tier).not.toBeEmpty();
});

test('classify-panel: lege textarea geeft foutmelding', async ({ page }) => {
  await page.goto(BASE);
  // Verwijder het required-attribuut zodat browser-validatie geen submit blokkeert;
  // de server-side check op missing scope_md moet 400 geven en het foutblok tonen.
  await page.locator('#classify-panel textarea').evaluate((el: HTMLTextAreaElement) => {
    el.removeAttribute('required');
  });
  await page.locator('#classify-panel button').click();
  await expect(page.locator('#classify-error')).not.toHaveAttribute('hidden', '', { timeout: 5000 });
});

test('dark mode: geen witte achtergrond + screenshot', async ({ page }) => {
  await page.emulateMedia({ colorScheme: 'dark' });
  await page.goto(BASE);
  // Wacht op eerste paint
  await page.waitForLoadState('networkidle');
  const bg = await page.evaluate(() =>
    getComputedStyle(document.body).backgroundColor
  );
  // Achtergrond mag niet rgb(255,255,255) zijn
  expect(bg).not.toBe('rgb(255, 255, 255)');
  await page.screenshot({
    path: path.join(SCREENSHOT_DIR, 'screenshot-dark.png'),
    fullPage: true,
  });
});

test('light mode: screenshot voor visuele referentie', async ({ page }) => {
  await page.emulateMedia({ colorScheme: 'light' });
  await page.goto(BASE);
  await page.waitForLoadState('networkidle');
  await page.screenshot({
    path: path.join(SCREENSHOT_DIR, 'screenshot-light.png'),
    fullPage: true,
  });
});
