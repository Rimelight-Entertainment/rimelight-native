# Backend migration plan

Status: decisions made (section 1), worker source copied (section 2 done in part). Nothing built, deployed or migrated. No Cloudflare account, D1 database, R2
bucket or Access application has been changed by this plan.

This plan takes the personal backend out of `idantity.me` and into this repository (`worker/`), so each person can
run their own instance (PROJECT.md, sections 0 and 3). It builds on the port already written in the idantity.me repo
(`docs/personal-backend-move.md` there), which says the code was ported but never built, deployed, migrated or run.

## 0. Where things stand

| Piece | Current home | Target home |
|---|---|---|
| Personal API routes (13 files), owner-scoped logic | `idantity.me/src/api/routes/*` | `worker/src/routes/*` |
| Drizzle schema (11 files), generated history | `idantity.me/src/db/schema/*`, `drizzle/0001_personal_backend.sql` | `worker/src/db/schema/*`, `worker/drizzle/` |
| Mail worker (20 files) and `MailboxDO` | `idantity.me/src/workers/inbox/**` | `worker/src/workers/inbox/**` |
| Access check, owner lookup | `idantity.me/src/api/lib/auth*.ts`, `owner.ts` | `worker/src/lib/` |
| Currency rates, trash cleanup | `idantity.me/src/lib/currency.ts`, `src/tasks/cleanup.ts` | `worker/src/lib/`, `worker/src/tasks/` |
| Live personal data | Personal D1 (personal.idantity.me) and/or idantity.me D1 | the owner's own D1 (open question R0.3) |

Frontend code for personal (`personal.idantity.me`) stays where it is until retirement (step 7).

## 1. Decisions the owner has to make first

These are listed in PROJECT.md section 4. The plan assumes the defaults in brackets.

- Q1 public repo: `Rimelight-Entertainment/rimelight-personal`, fresh history (decided).
- Q2 open-source licence: MIT (decided).
- Q4 keep the Cloudflare API token after setup for automatic updates [ask at each update].
- R0.3 live data: `personal.idantity.me` D1 (decided by the owner). The copy in idantity.me is not the source of truth.

Progress: `worker/` has the copied source, the new entry and `/api` mount (step 2.3), config and manifest, and `GET /api/meta` (step 2.4). Not yet installed, type-checked or tested.

## 2. Create `worker/` in this repo (no deployment)

1. Copy the files in section 0 into `worker/`. Keep the file names, so a diff against idantity.me stays readable.
2. Replace the idantity.me-only config with values the deploy step sets:
   - `PERSONAL_OWNER_ID`: a secret. The Access identity's stable id from the verified JWT replaces it (R1.2).
   - `TEAM_DOMAIN`, `POLICY_AUD`: text bindings written by the app at deploy time. `POLICY_AUD` stays a placeholder
     until the Access application exists; while it is a placeholder every request fails with 401, by design.
   - `DOMAINS`, `EMAIL_ADDRESSES`: set from the wizard, not in the repo.
3. Mount routes at `/api/*`. The Worker is the whole host now, so the `/api/personal/v1` rewrite goes.
4. Add `GET /meta` returning backend version, schema version and which optional modules are configured (R1.3).
5. Apply the bug fixes in the idantity.me doc (section 4 there). Each one gets a test before it is ported.
6. Build with `pnpm` inside `worker/` only. Run `pnpm db:generate` and compare its output with `drizzle/0000` before
   accepting it. Do not run a remote migration.

Exit criteria: `worker/` builds, its tests pass, and `GET /meta` answers locally with `wrangler dev`.

## 3. Shared types (R1.4)

Each route declares zod request and response schemas. A script writes them to `worker/schema/*.json`. The Rust side
generates types from those files and CI fails on drift. Until that exists, `crates/core` keeps its hand-written types
and the contract tests in `crates/core/tests` decode fixtures captured from the Worker's own tests.

## 4. Provisioning (R3) and first deploy to the owner's account

The app does this, not a person:

1. Create or reuse D1, R2 bucket, Worker with bindings and the Durable Object migration, `workers.dev` route, Access
   application and policy for the owner's email.
2. Apply `worker/drizzle/` to D1.
3. Write `POLICY_AUD` from the Access application, then `GET /meta` must answer 200 with the signed-in user.

Steps that the dashboard may still need (R0.1): creating the Zero Trust organisation. Record each one the API cannot
do; the wizard shows them as guided steps.

Run this first against a scratch Cloudflare account, never the owner's.

## 5. Move the owner's data (R5.1, R5.2)

1. Export from the source D1 read-only: every table as JSON plus the R2 objects.
2. Provision a fresh instance on the owner's Cloudflare account through the normal path (section 4).
3. Import into the empty instance. Keep the R2 keys, so old `cdn.idantity.me` links keep resolving (Q5), or rewrite
   them during import.
4. Check row counts per table against the export before touching anything else.

Rollback: the source data is untouched by steps 1–4. Keep it until step 6 is verified.

## 6. Remove the backend from idantity.me (R5.3)

Only after step 5 is verified:

1. Remove `/api/personal`, the personal routes and their Access application from idantity.me.
2. Remove `PERSONAL_BLOB`, `MailboxDO`, the email handler and the personal tables.
3. Keep `/api/contact` and the public site.
4. Confirm `idantity.me` builds and its tests pass without the personal code.

## 7. Retire the web app (R5.4)

Map every file in `personal.idantity.me` to its new home or to "dropped on purpose". The owner deletes the site.

## 8. Checks before calling it done

- Row counts match per table (step 5.4).
- The desktop and web apps sign in, read and write against the new instance.
- `RIMELIGHT_API_BASE` points at the new instance; no code path names idantity.me.
- One automated smoke test: setup against a scratch account, sign-in, data persists, update redeploys (PROJECT.md).

## Open questions this plan does not answer

- Whether the mail module ships in the first release (R4), since it needs an owner-owned domain.
- Whether `scheduled` (trash cleanup) gets a cron trigger in the worker. It has never been scheduled.

## 9. Bug-fix check (step 2.5), done by reading the copied code

Checked against the idantity.me move doc, section 4. Not built or run.

- Present: storage rename/move/delete and both upload field names; projects typed insert; todo and note owner scope
  with 404s; batch todo actions; health fasting end and durationHours; measurement and workout document payloads;
  grocery reset and clear; wishlist under both keys.
- Fixed here: `PATCH /health/fasting` updated by id only. It now also filters on the owner.
- Fixed here: `PATCH` and `DELETE /projects/cards` now only touch cards on the owner's boards (join through the board).
  `PATCH` also refuses a move into a list the owner does not have (404).
- Fixed here: `POST /projects/cards` checks that a new list sits on one of the owner's boards, and a new card sits in one of the owner's lists (404 otherwise).
- Open: nothing in step 2.5 is left open. None of it has been compiled or run.

## 10. First build of `worker/` (run in this session)

- `pnpm install`: passes. `pnpm-workspace.yaml` approves the `esbuild` and `workerd` build scripts, as idantity.me does.
- Type check (`tsc --noEmit`, after `cf workers types`): zero errors.
  - `tsconfig.json` was replaced: the copied one extended the frontend (solid) preset and did not fit a Worker.
  - `src/api/lib/access-jwt.ts` replaces `cfAccessIdentifier` from `@rimelight/cms`. The installed `@rimelight/cms`
    0.0.54 does not export it. The idantity.me code imports it too, so that code has never built either.
- Unit tests (`pnpm test`): 9 files, 49 tests, all pass.
- Drift check (`drizzle-kit generate` in a scratch copy): the table set matches once the hand-written migration's
  renames are applied (`financial_subscription` to `financial_bill`, and the two rebuilt tables). Column-level drift
  was not checked.
- Not run: `wrangler dev` and any deploy. Both need the Cloudflare account, which is not touched.

## 11. Test deploy (step 2, done)

Account: the owner's Cloudflare account. Everything created is new and separate from the live data.
- D1 `rimelight-personal-test` (uuid c5a2a1c2-…). Migration `0000_personal_backend.sql` applied; 27 personal tables.
- R2 `rimelight-personal-test`.
- Worker `rimelight-personal-test` at https://rimelight-personal-test.idantity.workers.dev, with the MailboxDO.
- Secret `PERSONAL_OWNER_ID` set at deploy time (a temporary secrets file, since deleted).
- `TEAM_DOMAIN` and `POLICY_AUD` are still placeholders, so the worker fails closed. Checked: `/api/meta` and
  `/api/todos` return 401 without a token, and unknown paths return 404.

Code changes made to get it to build:
- `package.json` import map: one explicit entry per `#` alias. The bundler ignored the fallback arrays.
- `src/workers/inbox/lib/schemas.ts`: the `@rimelight/i18n` call (a Vite-only runtime, and an undefined key) is now
  the literal English message.
- `wrangler` added as a dev dependency: `cf deploy` needs a dev-server package.
- `cloudflare.config.ts` now names the test resources (uuids and names, not secrets). Restore the placeholders
  before committing.

Not yet: an Access application for the test worker, so authenticated requests have not been tested; and the
data export and import.

## 12. Test Access application (step 2, done)

- Access application `rimelight-personal-test` on the workers.dev hostname, AUD 47829e90…577887, one allow policy
  for the owner's email, 24 hour session. The live `idantity.me personal API` application was not changed.
- Worker `TEAM_DOMAIN` and `POLICY_AUD` now set to the team and that AUD; redeployed.
- Without a login the edge answers 302 to the Access login page.
- Signed-in check still to do in a browser: `/api/meta` should return the backend version and module flags.

## 13. Data move, rehearsal on the test instance (step 5, done for D1)

- Tool: `worker/tools/data-move.mjs` with `export`, `import` and `verify`. Export only reads. Import refuses a target
  that has rows, inserts parents before children (from the foreign keys), and batches by size for the Windows
  command-line limit. Verify compares row counts per table.
- Rehearsal: exported the 27 tables from `personal-dot-idantity-dot-me` (read-only) and imported them into the test
  database `rimelight-personal-test`. All 27 row counts match. The scratch export was deleted afterwards.
- Counts moved: note 27, fasting 6, financial_bill 12, financial_planned_purchase 8, financial_account 3,
  financial_transaction 4, kanban card 3, list 3, board 1, and 1 each for habits, groceries, measurements, music,
  notebook, pets, watchlist, workout, kanban_custom_field. Empty: todo, storage_files, financial_budget,
  financial_debt, items, notebook_entry, wishlist_items.
- Not checked: row contents (only counts), and the R2 bucket. `storage_files` is empty, but the bucket may still hold
  objects (attachments, mailbox files) that have no row. Count them before cutover.
- Test-only cleanup: the test database was cleared once after a partial run. This was done on the test uuid only.
- The tool calls the `cf` CLI through its Node script, at a path hard-coded in `CF_BIN`. Override with the `CF_BIN`
  environment variable on another machine.
