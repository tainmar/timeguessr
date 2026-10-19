import { expect, request, test, type APIRequestContext } from "@playwright/test";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { address, closeDb, REMOTE, RUN_ID, waitForMails } from "./support";

// Edge cases that are awkward to reach through the UI. Local only (uses the log mail driver
// and the database to read magic links quickly).
test.skip(REMOTE, "local-only API checks");
test.describe.configure({ mode: "serial" });

const here = path.dirname(fileURLToPath(import.meta.url));
const fixture = (name: string) => fs.readFileSync(path.join(here, "..", "fixtures", name));
const baseURL = process.env.E2E_BASE_URL ?? "http://localhost:8090";

async function organizerContext(label: string): Promise<APIRequestContext> {
  const ctx = await request.newContext({ baseURL, extraHTTPHeaders: { Origin: baseURL } });
  const email = address(label);
  expect((await ctx.post("/api/auth/request", { data: { email } })).ok()).toBe(true);
  const [mail] = await waitForMails(email, (m) => /sign-in link/i.test(m.subject));
  const token = mail.text.match(/#token=([A-Za-z0-9_-]+)/)![1];
  expect((await ctx.post("/api/auth/verify", { data: { token } })).ok()).toBe(true);
  return ctx;
}

function upload(ctx: APIRequestContext, token: string, file: Buffer, fields: Record<string, string>) {
  return ctx.post(`/api/collect/${token}/photos`, {
    multipart: { ...fields, file: { name: "photo.jpg", mimeType: "image/jpeg", buffer: file } },
  });
}

let org: APIRequestContext;
let anon: APIRequestContext;
let gameId = "";
const tokens: string[] = [];
let code = "";
const truth: Record<string, { year: number; lat: number; lng: number }> = {};

test.beforeAll(async () => {
  org = await organizerContext("api-org");
  anon = await request.newContext({ baseURL });
});

test.afterAll(async () => {
  await org?.dispose();
  await anon?.dispose();
  await closeDb();
});

test("organizer endpoints require a session and ownership", async () => {
  expect((await anon.get("/api/games")).status()).toBe(401);
  const res = await org.post("/api/games", { data: { name: `API ${RUN_ID}` } });
  gameId = (await res.json()).id;
  const other = await organizerContext("api-other");
  expect((await other.get(`/api/games/${gameId}`)).status()).toBe(404);
  expect((await other.delete(`/api/games/${gameId}`)).status()).toBe(404);
  await other.dispose();
});

test("cross-site writes are rejected", async () => {
  const res = await org.post("/api/games", {
    data: { name: "evil" },
    headers: { Origin: "https://evil.example" },
  });
  expect(res.status()).toBe(403);
});

test("collection: uploads are validated, capped at n even when racing, and stripped of EXIF", async () => {
  await org.post(`/api/games/${gameId}/participants`, {
    data: { entries: `P1 <${address("api-p1")}>, P2 <${address("api-p2")}>` },
  });
  const deadline = new Date(Date.now() + 2 * 24 * 3600_000).toISOString();
  const start = await org.post(`/api/games/${gameId}/start`, {
    data: { deadline_at: deadline, timezone: "Europe/Paris", reminder_days: [1] },
  });
  expect(start.ok()).toBe(true);
  const detail = await (await org.get(`/api/games/${gameId}`)).json();
  // The organizer is listed too; keep P1 and P2 in a known order.
  expect(detail.participants).toHaveLength(3);
  expect(detail.participants.some((p: { email: string }) => p.email === address("api-org"))).toBe(true);
  for (const label of ["api-p1", "api-p2"]) {
    const p = detail.participants.find((x: { email: string }) => x.email === address(label));
    tokens.push(p.collect_url.split("/c/")[1]);
  }

  // Validation
  const badYear = await upload(anon, tokens[0], fixture("photo-a1.jpg"), { year: "1700", lat: "1", lng: "1" });
  expect(badYear.status()).toBe(400);
  const notImage = await upload(anon, tokens[0], Buffer.from("definitely not an image"), { year: "2000", lat: "1", lng: "1" });
  expect(notImage.status()).toBe(415);
  const huge = Buffer.alloc(22 * 1024 * 1024, 1);
  const tooBig = await upload(anon, tokens[0], huge, { year: "2000", lat: "1", lng: "1" });
  expect(tooBig.status()).toBe(413);

  // Three concurrent uploads for n = 1: exactly one wins.
  const racing = await Promise.all(
    [0, 1, 2].map(() => upload(anon, tokens[0], fixture("photo-a1.jpg"), { year: "1995", lat: "48.8584", lng: "2.2945" })),
  );
  expect(racing.map((r) => r.status()).sort()).toEqual([200, 409, 409]);
  const photo = (await racing.find((r) => r.ok())!.json()).photo;
  truth[photo.id] = { year: 1995, lat: 48.8584, lng: 2.2945 };

  // The served file is a fresh JPEG without the original EXIF (GPS, date).
  const image = await (await anon.get(`/api/images/${photo.id}`)).body();
  expect(image.subarray(0, 2)).toEqual(Buffer.from([0xff, 0xd8]));
  expect(image.includes(Buffer.from("Exif\0\0"))).toBe(false);
  expect(fixture("photo-a1.jpg").includes(Buffer.from("Exif\0\0"))).toBe(true);

  // Longitude outside [-180, 180) is normalized rather than rejected.
  const p2 = await upload(anon, tokens[1], fixture("photo-b1.jpg"), { year: "1988", lat: "-33.8568", lng: `${151.2153 - 360}` });
  expect(p2.ok()).toBe(true);
  const p2photo = (await p2.json()).photo;
  expect(p2photo.lng).toBeCloseTo(151.2153, 4);
  truth[p2photo.id] = { year: 1988, lat: -33.8568, lng: 151.2153 };

  const view = await (await anon.get(`/api/collect/${tokens[0]}`)).json();
  expect(view.participant.status).toBe("ready");
  expect(view.can_upload).toBe(false);
});

test("play: no spoilers, exact and antipodal scores, idempotent guesses", async () => {
  const ready = await org.post(`/api/games/${gameId}/ready`, { data: { notify_participants: false } });
  expect(ready.ok()).toBe(true);
  code = (await ready.json()).play_url.split("/play/")[1];

  // Uploads are closed once the game is ready.
  expect((await upload(anon, tokens[1], fixture("photo-c1.jpg"), { year: "2000", lat: "1", lng: "1" })).status()).toBe(409);

  const join = await (await anon.post(`/api/play/${code}/teams`, { data: { name: "Exact" } })).json();
  const T = { "X-Team-Token": join.token };
  const info = await (await anon.get(`/api/play/${code}`)).json();
  expect(info.year_min).toBe(1988 - 5);
  expect(info.year_max).toBe(new Date().getUTCFullYear());

  const state = await (await anon.get(`/api/play/${code}/state`, { headers: T })).json();
  const raw = JSON.stringify(state);
  for (const key of ['"lat"', '"lng"', '"caption"', '"owner"', '"answer"']) expect(raw).not.toContain(key);

  const first = state.round.photo_id as string;
  const second = Object.keys(truth).find((id) => id !== first)!;

  // Only the current round can be answered.
  const skip = await anon.post(`/api/play/${code}/guess`, { headers: T, data: { photo_id: second, year: 2000, lat: 0, lng: 0 } });
  expect(skip.status()).toBe(409);

  const exact = { photo_id: first, ...truth[first] };
  const r1 = await (await anon.post(`/api/play/${code}/guess`, { headers: T, data: exact })).json();
  expect(r1.year_score).toBe(500);
  expect(r1.location_score).toBe(500);
  expect(r1.score).toBe(1000);
  const again = await (await anon.post(`/api/play/${code}/guess`, { headers: T, data: { ...exact, year: 1900 } })).json();
  expect(again.score).toBe(1000); // the first answer stands

  const t = truth[second];
  const antipode = { photo_id: second, year: t.year + 5, lat: -t.lat, lng: t.lng + 180 };
  const r2 = await (await anon.post(`/api/play/${code}/guess`, { headers: T, data: antipode })).json();
  expect(r2.location_score).toBe(0);
  expect(r2.year_score).toBe(250);
  expect(r2.total_score).toBe(1250);
  expect(r2.finished).toBe(true);

  // A team still playing sees no answers on the leaderboard; the finished team does.
  const other = await (await anon.post(`/api/play/${code}/teams`, { data: { name: "Late" } })).json();
  const theirs = await (await anon.get(`/api/play/${code}/leaderboard`, { headers: { "X-Team-Token": other.token } })).json();
  expect(theirs.reveal_answers).toBe(false);
  expect(JSON.stringify(theirs.photos)).not.toContain('"answer"');
  const mine = await (await anon.get(`/api/play/${code}/leaderboard`, { headers: T })).json();
  expect(mine.reveal_answers).toBe(true);
  expect(mine.teams.find((x: { name: string }) => x.name === "Exact").total).toBe(1250);
});

test("team PINs: required to rejoin, brute force is throttled", async () => {
  const team = await (await anon.post(`/api/play/${code}/teams`, { data: { name: "Locked" } })).json();
  expect((await anon.post(`/api/play/${code}/teams`, { data: { name: "LOCKED" } })).status()).toBe(409);
  const wrong = team.team.pin === "0000" ? "1111" : "0000";
  for (let i = 0; i < 8; i++) {
    expect((await anon.post(`/api/play/${code}/teams`, { data: { name: "Locked", pin: wrong } })).status()).toBe(403);
  }
  // Even the right PIN is refused while locked.
  expect((await anon.post(`/api/play/${code}/teams`, { data: { name: "Locked", pin: team.team.pin } })).status()).toBe(429);
});

test("magic links are single use and sign-in requests are throttled", async () => {
  const ctx = await request.newContext({ baseURL });
  const email = address("api-throttle");
  for (let i = 0; i < 5; i++) expect((await ctx.post("/api/auth/request", { data: { email } })).ok()).toBe(true);
  expect((await ctx.post("/api/auth/request", { data: { email } })).status()).toBe(429);
  const [mail] = await waitForMails(email, (m) => /sign-in link/i.test(m.subject));
  const token = mail.text.match(/#token=([A-Za-z0-9_-]+)/)![1];
  expect((await ctx.post("/api/auth/verify", { data: { token } })).ok()).toBe(true);
  expect((await ctx.post("/api/auth/verify", { data: { token } })).status()).toBe(400);
  await ctx.dispose();
});
