import { expect, type Locator, type Page } from "@playwright/test";
import pg from "pg";

export const REMOTE = !!process.env.E2E_REMOTE;
export const RUN_ID = Date.now().toString(36);

/** Unique address per run. Remote runs use Resend's test inbox (delivered, never bounces). */
export function address(label: string): string {
  return REMOTE ? `delivered+${label}-${RUN_ID}@resend.dev` : `${label}-${RUN_ID}@example.com`;
}

export interface Mail {
  to: string;
  subject: string;
  html: string;
  text: string;
  kind?: string;
}

let pool: pg.Pool | null = null;
export function db(): pg.Pool {
  if (REMOTE) throw new Error("No database access in remote mode");
  pool ??= new pg.Pool({
    connectionString: process.env.E2E_DATABASE_URL ?? "postgres://postgres@localhost:5433/timeguessr_e2e",
  });
  return pool;
}

export async function closeDb() {
  await pool?.end();
  pool = null;
}

async function localMails(to: string): Promise<Mail[]> {
  const res = await db().query(
    "SELECT to_email AS to, subject, html, text_body AS text, kind FROM emails WHERE lower(to_email) = lower($1) AND status = 'sent' ORDER BY created_at",
    [to],
  );
  return res.rows;
}

async function resend<T>(path: string): Promise<T> {
  const key = process.env.RESEND_API_KEY;
  if (!key) throw new Error("RESEND_API_KEY is required for remote E2E runs");
  for (let attempt = 0; ; attempt++) {
    const res = await fetch(`https://api.resend.com${path}`, { headers: { Authorization: `Bearer ${key}` } });
    if (res.status === 429 && attempt < 5) {
      await new Promise((r) => setTimeout(r, 1500));
      continue;
    }
    if (!res.ok) throw new Error(`Resend ${path}: ${res.status} ${await res.text()}`);
    return res.json() as Promise<T>;
  }
}

async function remoteMails(to: string): Promise<Mail[]> {
  const list = await resend<{ data: { id: string; to: string[]; subject: string }[] }>("/emails?limit=100");
  const mine = list.data.filter((e) => e.to.some((t) => t.toLowerCase() === to.toLowerCase())).reverse();
  const out: Mail[] = [];
  for (const e of mine) {
    const full = await resend<{ html: string; text: string; subject: string }>(`/emails/${e.id}`);
    out.push({ to, subject: full.subject, html: full.html ?? "", text: full.text ?? "" });
  }
  return out;
}

/** All emails delivered to an address, oldest first. */
export function mailsTo(to: string): Promise<Mail[]> {
  return REMOTE ? remoteMails(to) : localMails(to);
}

/** Wait until `count` emails matching `filter` reached `to`; returns them. */
export async function waitForMails(
  to: string,
  filter: (m: Mail) => boolean,
  count = 1,
  timeout = REMOTE ? 90_000 : 20_000,
): Promise<Mail[]> {
  let found: Mail[] = [];
  await expect
    .poll(
      async () => {
        found = (await mailsTo(to)).filter(filter);
        return found.length;
      },
      { timeout, intervals: [500, 1000, 2000, 3000] },
    )
    .toBeGreaterThanOrEqual(count);
  return found;
}

export function linkIn(mail: Mail, pattern: RegExp): string {
  const match = mail.text.match(pattern) ?? mail.html.match(pattern);
  if (!match) throw new Error(`No link matching ${pattern} in "${mail.subject}"`);
  return match[0];
}

/** Sign in an organizer through the real magic link flow. */
export async function signIn(page: Page, email: string) {
  await page.goto("/");
  await page.getByLabel(/sign in with your email/i).fill(email);
  await page.getByRole("button", { name: /send me a link/i }).click();
  await expect(page.getByTestId("link-sent")).toBeVisible();
  const [mail] = await waitForMails(email, (m) => /sign-in link/i.test(m.subject));
  const link = linkIn(mail, /https?:\/\/[^\s"<]+\/auth\/verify#token=[A-Za-z0-9_-]+/);
  await page.goto(link.replace(/^https?:\/\/[^/]+/, ""));
  await expect(page.getByRole("heading", { name: "My games" })).toBeVisible();
}

/** Wait for a MapLibre map to finish loading its style. */
export async function mapReady(map: Locator) {
  await expect(map).toHaveAttribute("data-loaded", "1", { timeout: 30_000 });
}

/** Click a map at a relative position (0..1) of its canvas. */
export async function clickMap(map: Locator, fx = 0.5, fy = 0.5) {
  await map.scrollIntoViewIfNeeded();
  const box = await map.boundingBox();
  if (!box) throw new Error("map not visible");
  await map.page().mouse.click(box.x + box.width * fx, box.y + box.height * fy);
}

/** Pin a precise location on the upload map: taps zoom in until the pin can be placed. */
export async function pinLocation(page: Page, fx = 0.5, fy = 0.5) {
  const map = page.getByTestId("picker-map");
  await mapReady(map);
  await map.scrollIntoViewIfNeeded();
  for (let i = 0; i < 6; i++) {
    await clickMap(map, fx, fy);
    await page.waitForTimeout(900);
    if (await page.getByText(/^Pin at /).isVisible()) return;
  }
  throw new Error("could not place the pin");
}

/** datetime-local value for a date, in the browser's local time zone (tests run in UTC). */
export function localInput(d: Date): string {
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}T${pad(d.getHours())}:${pad(d.getMinutes())}`;
}
