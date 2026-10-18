import { devices, expect, test, type Browser, type BrowserContext, type Page } from "@playwright/test";
import path from "node:path";
import { fileURLToPath } from "node:url";
import {
  address,
  clickMap,
  closeDb,
  db,
  linkIn,
  localInput,
  mapReady,
  pinLocation,
  REMOTE,
  RUN_ID,
  signIn,
  snap,
  waitForMails,
} from "./support";

const here = path.dirname(fileURLToPath(import.meta.url));
const fixture = (name: string) => path.join(here, "..", "fixtures", name);

const GAME = `E2E Offsite ${RUN_ID}`;
const organizerEmail = address("organizer");
const alice = address("alice");
const bob = address("bob");
const carol = address("carol");

test.describe.configure({ mode: "serial" });

let browser: Browser;
let organizer: Page;
let gameUrl = "";
const collectLinks: Record<string, string> = {};
let playPath = "";
const totals: Record<string, number> = {};

test.beforeAll(async ({ browser: b }) => {
  browser = b;
  const ctx = await browser.newContext();
  organizer = await ctx.newPage();
});

test.afterAll(async () => {
  if (!REMOTE) await closeDb();
});

const relative = (url: string) => url.replace(/^https?:\/\/[^/]+/, "");

async function newPage(options: Parameters<Browser["newContext"]>[0] = {}): Promise<[BrowserContext, Page]> {
  const ctx = await browser.newContext(options);
  return [ctx, await ctx.newPage()];
}

/** Device emulation (viewport, touch, user agent) on the Chromium engine. */
function device(name: string) {
  const { defaultBrowserType: _ignored, ...options } = devices[name];
  return options;
}

test("organizer signs in with a magic link and creates a game", async () => {
  await signIn(organizer, organizerEmail);
  await organizer.getByLabel(/What's your name/).fill("Morgan from HR");
  await organizer.getByRole("button", { name: "Save" }).click();
  await expect(organizer.getByLabel(/What's your name/)).toBeHidden();
  await organizer.getByLabel("New game").fill(GAME);
  await organizer.getByRole("button", { name: "Create game" }).click();
  await expect(organizer.getByTestId("game-title")).toContainText(GAME);
  gameUrl = new URL(organizer.url()).pathname;

  await organizer.getByTestId("photos-per-participant").selectOption("2");
  await organizer.getByLabel(/Message to participants/).fill("Dig out your old photos!");
  await organizer.getByRole("button", { name: "Save settings" }).click();
  await expect(organizer.getByText("Settings saved")).toBeVisible();

  await organizer
    .getByTestId("participants-input")
    .fill(`Alice <${alice}>\n${bob}, ${carol}\nnot-an-email`);
  await organizer.getByRole("button", { name: "Add participants" }).click();
  await expect(organizer.getByTestId("participant-row")).toHaveCount(3);
  await expect(organizer.getByText(/ignored invalid: not-an-email/)).toBeVisible();
});

test("starting the collection emails every participant with the pending count", async () => {
  const deadline = new Date(Date.now() + 3 * 24 * 3600_000);
  deadline.setHours(18, 0, 0, 0);
  await organizer.getByTestId("deadline-input").fill(localInput(deadline));
  // Default reminders are D-3 and D-1; add D-2 as well.
  await organizer.getByLabel(/Custom reminder/).fill("2");
  await organizer.getByRole("button", { name: "Add", exact: true }).click();
  await organizer.getByTestId("start-collecting").click();
  await expect(organizer.getByTestId("collection-card")).toBeVisible();
  await expect(organizer.getByTestId("collection-progress")).toContainText("0/3");

  for (const [who, email] of Object.entries({ alice, bob, carol })) {
    const [mail] = await waitForMails(email, (m) => /invited you to/.test(m.subject));
    expect(mail.subject).toContain(GAME);
    expect(mail.subject).toContain("Morgan from HR invited you");
    expect(mail.text).toContain("3 of 3 participants haven't finished uploading yet");
    expect(mail.text).toContain("Dig out your old photos!");
    collectLinks[who] = relative(linkIn(mail, /https?:\/\/[^\s"<]+\/c\/[A-Za-z0-9_-]+/));
  }
});

test("Alice uploads her two photos (EXIF suggestions, GPS, map pin) and becomes ready", async () => {
  const [ctx, page] = await newPage(device("Desktop Chrome"));
  await page.goto(collectLinks.alice);
  await expect(page.getByTestId("collect-title")).toHaveText(GAME);
  await expect(page.getByTestId("pending-stats")).toContainText("3 of 3 participants");
  await expect(page.getByTestId("my-name")).toHaveValue("Alice");

  // Photo 1: EXIF date is only a suggestion; use the GPS position stored in the file.
  await page.getByTestId("file-input").setInputFiles(fixture("photo-a1.jpg"));
  await expect(page.getByRole("button", { name: /The file says 2001/ })).toBeVisible();
  await page.getByTestId("year-input").fill("1995");
  await page.getByRole("button", { name: /Use the location stored in the photo/ }).click();
  await expect(page.getByText(/^Pin at 48\.858/)).toBeVisible();
  await page.getByTestId("caption-input").fill("Eiffel tower trip");
  await page.waitForTimeout(1500);
  await snap(page, "03-collect-form-desktop");
  await page.getByTestId("save-photo").click();
  await expect(page.getByText("Photo saved!")).toBeVisible();
  await expect(page.getByTestId("my-photo")).toHaveCount(1);
  await expect(page.getByTestId("participant-status")).toHaveText("Partial");

  // Photo 2: pin by tapping the map (taps zoom in until the pin can be placed precisely).
  await page.getByTestId("file-input").setInputFiles(fixture("photo-a2.jpg"));
  await page.getByTestId("year-input").fill("2008");
  await expect(page.getByTestId("caption-input")).toHaveValue("");
  await expect(page.getByText(/^Pin at /)).toBeHidden();
  await pinLocation(page, 0.55, 0.45);
  await page.getByTestId("save-photo").click();
  await expect(page.getByTestId("my-photo")).toHaveCount(2);
  await expect(page.getByTestId("participant-status")).toHaveText("Ready");
  await expect(page.getByTestId("all-done")).toBeVisible();
  await snap(page, "04-collect-done-desktop");
  await ctx.close();
});

test("Bob uploads one photo (place search) and declares he's done", async () => {
  const [ctx, page] = await newPage(device("Pixel 7"));
  await page.goto(collectLinks.bob);
  await page.getByTestId("my-name").fill("Bob");
  await page.locator("form.name-card").getByRole("button", { name: "Save" }).click();
  await expect(page.getByText("Name saved")).toBeVisible();

  await page.getByTestId("file-input").setInputFiles(fixture("photo-b1.jpg"));
  await page.getByTestId("year-input").fill("1988");
  await mapReady(page.getByTestId("picker-map"));
  await page.getByTestId("place-search").fill("Sydney Opera House");
  await page.getByRole("button", { name: "Search" }).click();
  const result = page.locator(".search-results button").first();
  const found = await result
    .waitFor({ state: "visible", timeout: 10_000 })
    .then(() => true)
    .catch(() => false);
  if (found) {
    await result.click();
    await expect(page.getByText(/Pin placed/)).toBeVisible();
  } else {
    // Nominatim unreachable: fall back to tapping the map.
    await pinLocation(page, 0.6, 0.6);
  }
  await page.getByTestId("save-photo").click();
  await expect(page.getByTestId("my-photo")).toHaveCount(1);
  await expect(page.getByTestId("participant-status")).toHaveText("Partial");

  await page.getByTestId("im-done").click();
  await expect(page.getByTestId("participant-status")).toHaveText("Ready (done)");
  await expect(page.getByTestId("photo-form")).toBeHidden();
  await snap(page, "05-collect-mobile");
  await ctx.close();
});

test("Carol uploads only one photo and stays partial", async () => {
  const [ctx, page] = await newPage(device("iPhone 13"));
  await page.goto(collectLinks.carol);
  await page.getByTestId("my-name").fill("Carol");
  await page.locator("form.name-card").getByRole("button", { name: "Save" }).click();
  await page.getByTestId("file-input").setInputFiles(fixture("photo-c1.jpg"));
  await page.getByTestId("year-input").fill("2015");
  await pinLocation(page, 0.35, 0.4);
  await page.getByTestId("save-photo").click();
  await expect(page.getByTestId("my-photo")).toHaveCount(1);
  await expect(page.getByTestId("participant-status")).toHaveText("Partial");
  await ctx.close();
});

test("organizer sees statuses and in-app notifications", async () => {
  await organizer.reload();
  const row = (name: string) => organizer.getByTestId("participant-row").filter({ hasText: name });
  await expect(row("Alice").getByTestId("participant-status")).toHaveText("Ready");
  await expect(row("Bob").getByTestId("participant-status")).toHaveText("Ready (done)");
  await expect(row("Carol").getByTestId("participant-status")).toHaveText("Partial");
  await expect(organizer.getByTestId("collection-progress")).toContainText("2/3");
  await expect(organizer.getByTestId("collection-progress")).toContainText("4 photos");

  await expect(organizer.getByTestId("bell-count")).toBeVisible();
  await organizer.getByTestId("bell").click();
  const panel = organizer.getByTestId("bell-panel");
  await expect(panel).toContainText("Alice is ready");
  await expect(panel).toContainText("Bob is ready");
  await expect(panel).toContainText("Carol uploaded a photo");
  await snap(organizer, "06-organizer-collecting", false);
  await panel.getByRole("button", { name: "Mark all as read" }).click();
  await expect(organizer.getByTestId("bell-count")).toBeHidden();
  await organizer.keyboard.press("Escape");
  await organizer.mouse.click(5, 300);
});

test("a due reminder only goes to participants who aren't ready, with the pending count", async () => {
  test.skip(REMOTE, "needs database access to fast-forward the clock");
  const gameId = gameUrl.split("/").pop();
  const { rowCount } = await db().query(
    "UPDATE reminders SET send_at = now() - interval '1 second' WHERE game_id = $1 AND status = 'scheduled'",
    [gameId],
  );
  expect(rowCount).toBeGreaterThan(0);
  const [mail] = await waitForMails(carol, (m) => /^Reminder/.test(m.subject));
  expect(mail.text).toContain("1 of 3 participants hasn't finished uploading yet");
  expect(mail.text).toContain("You've shared 1 of 2 photos so far");
  // Overdue reminders are collapsed into one, and ready participants get nothing.
  await organizer.waitForTimeout(2000);
  expect((await waitForMails(carol, (m) => /^Reminder/.test(m.subject))).length).toBe(1);
  const { rows } = await db().query(
    "SELECT count(*)::int AS n FROM emails WHERE kind = 'reminder' AND game_id = $1 AND lower(to_email) <> lower($2)",
    [gameId, carol],
  );
  expect(rows[0].n).toBe(0);
});

test("marking the game ready creates the play link and emails it", async () => {
  await organizer.reload();
  organizer.once("dialog", (d) => d.accept());
  await organizer.getByTestId("mark-ready").click();
  await expect(organizer.getByTestId("play-card")).toBeVisible();
  const playUrl = await organizer.getByTestId("play-url").textContent();
  expect(playUrl).toMatch(/\/play\/[a-z0-9]{10}$/);
  playPath = relative(playUrl!);
  await snap(organizer, "07-organizer-ready");

  const [mail] = await waitForMails(alice, (m) => /ready to play/.test(m.subject));
  expect(relative(linkIn(mail, /https?:\/\/[^\s"<]+\/play\/[a-z0-9]+/))).toBe(playPath);
  await waitForMails(carol, (m) => /ready to play/.test(m.subject));

  // Uploads are closed now.
  const [ctx, page] = await newPage();
  await page.goto(collectLinks.carol);
  await expect(page.getByText("Uploads are closed")).toBeVisible();
  await ctx.close();
});

async function playRound(page: Page, opts: { touch?: boolean; zoom?: boolean; yearSteps: number; fx: number; shots?: string }) {
  await expect(page.getByTestId("round")).toBeVisible();
  if (opts.zoom) {
    const photo = page.getByTestId("round-photo");
    const content = photo.locator(".zoomable-content");
    const before = await content.evaluate((el) => getComputedStyle(el).transform);
    const box = (await photo.boundingBox())!;
    await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
    await page.mouse.wheel(0, -600);
    await expect.poll(() => content.evaluate((el) => getComputedStyle(el).transform)).not.toBe(before);
    await page.getByRole("button", { name: "Reset zoom" }).click();
  }
  const slider = page.getByTestId("year-slider");
  const start = Number(await page.getByTestId("year-value").textContent());
  await slider.focus();
  for (let i = 0; i < Math.abs(opts.yearSteps); i++) await page.keyboard.press(opts.yearSteps < 0 ? "ArrowLeft" : "ArrowRight");
  await expect(page.getByTestId("year-value")).toHaveText(String(start + opts.yearSteps));

  const map = page.getByTestId("guess-map");
  await mapReady(map);
  await map.scrollIntoViewIfNeeded();
  if (opts.touch) {
    const box = (await map.boundingBox())!;
    await page.touchscreen.tap(box.x + box.width * opts.fx, box.y + box.height * 0.5);
  } else {
    await clickMap(map, opts.fx, 0.5);
  }
  await expect(page.getByTestId("submit-guess")).toBeEnabled();
  if (opts.shots) await snap(page, `${opts.shots}-round`, false);
  await page.getByTestId("submit-guess").click();
  await expect(page.getByTestId("reveal")).toBeVisible();
  const score = Number((await page.getByTestId("round-score").textContent())!.replace("+", ""));
  expect(score).toBeGreaterThanOrEqual(0);
  expect(score).toBeLessThanOrEqual(1000);
  await mapReady(page.getByTestId("reveal-map"));
  if (opts.shots) {
    await page.waitForTimeout(1500);
    await snap(page, `${opts.shots}-reveal`, false);
  }
  return score;
}

test("team Alpha plays all rounds on a desktop (zoom, slider, map) and sees the leaderboard", async () => {
  const [, page] = await newPage();
  await page.goto(playPath);
  await expect(page.getByTestId("play-title")).toHaveText(GAME);
  await page.getByTestId("team-name").fill("Team Alpha");
  await page.getByTestId("join").click();
  await expect(page.getByTestId("pin-banner")).toBeVisible();

  let total = 0;
  for (let round = 1; round <= 4; round++) {
    await expect(page.getByTestId("round-counter")).toHaveText(`Photo ${round} / 4`);
    total += await playRound(page, {
      zoom: round === 1,
      yearSteps: -3 - round,
      fx: 0.3 + round * 0.1,
      shots: round === 1 ? "08-desktop" : undefined,
    });
    await expect(page.getByTestId("total-score")).toHaveText(`${total.toLocaleString("en-US")} pts`);
    await page.getByTestId("next").click();
  }
  totals.alpha = total;
  await expect(page).toHaveURL(new RegExp(`${playPath}/leaderboard$`));
  const row = page.getByTestId("leaderboard-row").filter({ hasText: "Team Alpha" });
  await expect(row.getByTestId("team-total")).toHaveText(total.toLocaleString("en-US"));
  // keep this page open: it is the "projector" for the auto-refresh check
  (globalThis as Record<string, unknown>).alphaLeaderboard = page;
});

test("team Beta plays on two phones sharing the team via its PIN", async () => {
  const [, phone1] = await newPage(device("Pixel 7"));
  await phone1.goto(playPath);
  await phone1.getByTestId("team-name").fill("Team Beta");
  await phone1.getByTestId("join").click();
  await expect(phone1.getByTestId("pin-banner")).toBeVisible();
  const pin = (await phone1.getByTestId("pin-banner").locator(".pin").textContent())!.trim();
  expect(pin).toMatch(/^\d{4}$/);

  let total = 0;
  total += await playRound(phone1, { touch: true, yearSteps: 2, fx: 0.7, shots: "09-mobile" });
  await phone1.getByTestId("next").click();

  // A second phone joins the same team with the PIN and continues where the team is.
  const [, phone2] = await newPage(device("Pixel 7"));
  await phone2.goto(playPath);
  await phone2.getByTestId("team-name").fill("team beta");
  await phone2.getByTestId("join").click();
  await expect(phone2.getByTestId("team-pin")).toBeVisible();
  await phone2.getByTestId("team-pin").fill("0000" === pin ? "1111" : "0000");
  await phone2.getByTestId("join").click();
  await expect(phone2.getByText("Wrong PIN for this team.")).toBeVisible();
  await phone2.getByTestId("team-pin").fill(pin);
  await phone2.getByTestId("join").click();
  await expect(phone2.getByTestId("round-counter")).toHaveText("Photo 2 / 4");

  const alpha = (globalThis as Record<string, unknown>).alphaLeaderboard as Page;
  const betaRow = alpha.getByTestId("leaderboard-row").filter({ hasText: "Team Beta" });

  for (let round = 2; round <= 4; round++) {
    total += await playRound(phone2, { touch: true, yearSteps: round, fx: 0.25 + round * 0.12 });
    await phone2.getByTestId("next").click();
    if (round === 2) {
      // The projector leaderboard picks up the new team by itself (10 s auto-refresh).
      await expect(betaRow).toContainText("playing", { timeout: 15_000 });
    }
  }
  totals.beta = total;
  await expect(phone2).toHaveURL(new RegExp(`${playPath}/leaderboard$`));
  await expect(betaRow.getByTestId("team-total")).toHaveText(total.toLocaleString("en-US"), { timeout: 15_000 });
  await snap(alpha, "10-leaderboard");
  await snap(phone2, "11-leaderboard-mobile");
});

test("leaderboard photos open in a zoomable lightbox that survives auto-refresh", async () => {
  const page = (globalThis as Record<string, unknown>).alphaLeaderboard as Page;
  await expect(page.getByTestId("leaderboard-row")).toHaveCount(2);
  const first = page.getByTestId("leaderboard-row").first();
  const expectedLeader = totals.alpha >= totals.beta ? "Team Alpha" : "Team Beta";
  await expect(first).toContainText(expectedLeader);

  await page.getByTestId("leaderboard-thumb").first().click();
  const lightbox = page.getByTestId("lightbox");
  await expect(lightbox).toBeVisible();
  // Alpha has finished, so answers and every team's guesses are revealed.
  await expect(page.getByTestId("photo-details")).toContainText(/\d{4} · (Alice|Bob|Carol)/);
  await expect(page.getByTestId("photo-details")).toContainText("Team Beta");

  const content = lightbox.locator(".zoomable-content");
  const initial = await content.evaluate((el) => getComputedStyle(el).transform);
  const box = (await lightbox.getByTestId("lightbox-zoom").boundingBox())!;
  await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
  await page.mouse.wheel(0, -800);
  await expect.poll(() => content.evaluate((el) => getComputedStyle(el).transform)).not.toBe(initial);
  await page.waitForTimeout(500);
  const zoomed = await content.evaluate((el) => getComputedStyle(el).transform);
  await snap(page, "12-lightbox", false);
  await page.waitForTimeout(11_000); // one auto-refresh later...
  await expect(lightbox).toBeVisible(); // ...the lightbox is still open
  expect(await content.evaluate((el) => getComputedStyle(el).transform)).toBe(zoomed);
  await page.getByTestId("lightbox-close").click();
  await expect(lightbox).toBeHidden();
});

test("anonymous viewers get no answers until the organizer ends the game", async () => {
  const [ctx, page] = await newPage();
  const res = await page.request.get(`/api${playPath}/leaderboard`);
  const before = await res.json();
  expect(before.reveal_answers).toBe(false);
  expect(before.photos.every((p: Record<string, unknown>) => !("answer" in p))).toBe(true);

  await organizer.goto(gameUrl);
  organizer.once("dialog", (d) => d.accept());
  await organizer.getByTestId("end-game").click();
  await expect(organizer.getByText("Game ended: answers revealed")).toBeVisible();

  await page.goto(`${playPath}/leaderboard`);
  await expect(page.getByText("Game over: answers revealed")).toBeVisible();
  await expect(page.getByTestId("leaderboard-thumb").first()).toContainText(/^\d{4}$/);
  await ctx.close();
});
