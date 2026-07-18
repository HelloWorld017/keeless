import { resolve } from 'node:path';
import { chromium } from 'playwright-core';

const executablePath = process.env.CHROMIUM_PATH;
if (!executablePath) {
  throw new Error('CHROMIUM_PATH must point to a Chromium executable');
}

const browser = await chromium.launch({
  executablePath,
  headless: true,
  args: ['--no-sandbox', '--disable-dev-shm-usage'],
});
const page = await browser.newPage();
const browserErrors = [];
page.on('console', message => {
  if (message.type() === 'error') {
    browserErrors.push(`console: ${message.text()}`);
  }
});
page.on('pageerror', error => browserErrors.push(`page: ${error.message}`));

try {
  await page.goto(`${process.env.KEELESS_BASE_URL ?? 'http://127.0.0.1:4173'}/open`);
  await page.getByText('Import another database').waitFor({ timeout: 15_000 });

  const fixture = resolve(
    process.cwd(),
    '../kdbx/tests/resources/test_db_kdbx4_with_password_aes.kdbx',
  );
  await page.locator('input[type="file"]').setInputFiles(fixture);
  await page.getByLabel('Master password').waitFor({ timeout: 15_000 });

  await page.getByLabel('Master password').fill('not-the-password');
  await page.getByRole('button', { name: 'Unlock database' }).click();
  await page
    .getByText('That password could not unlock this database.')
    .waitFor({ timeout: 15_000 });

  await page.getByLabel('Master password').fill('demopass');
  await page.getByRole('button', { name: 'Unlock database' }).click();
  await page.getByText('Database unlocked').waitFor({ timeout: 30_000 });

  await page.reload();
  await page.getByRole('button', { name: /test_db_kdbx4_with_password_aes\.kdbx/u }).click();
  await page.getByLabel('Master password').fill('demopass');
  await page.getByRole('button', { name: 'Unlock database' }).click();
  await page.getByText('Database unlocked').waitFor({ timeout: 30_000 });

  if (browserErrors.length > 0) {
    throw new Error(browserErrors.join('\n'));
  }
  console.info('Browser host handshake and database unlock passed.');
} catch (error) {
  if (browserErrors.length > 0) {
    console.error(browserErrors.join('\n'));
  }
  throw error;
} finally {
  await browser.close();
}
