# Rimelight Personal worker

The backend for Rimelight Personal: a Cloudflare Worker with D1 (SQLite), R2 (files and mail attachments) and a
Durable Object that holds each mailbox. One worker serves one person's data.

Status: source copied from `personal.idantity.me`'s backend port (idantity.me repo). Not built, deployed or
migrated. See `../docs/MIGRATION.md`.

## Layout

- `src/fetch.ts`: the Worker entry. Exports the mail `email` handler and `MailboxDO`.
- `src/api/server.ts`: the `/api` routes, all behind a Cloudflare Access check.
- `src/api/routes/`: one file per module (todos, notes, finances, ...).
- `src/db/`: Drizzle schema for the personal tables.
- `src/workers/inbox/`: mail API, storage and the mailbox Durable Object.
- `drizzle/0000_personal_backend.sql`: the hand-written migration for the personal tables.

## Deploy-time values

`cloudflare.config.ts` holds placeholders (`REPLACE_AT_DEPLOY`). The app sets the account, the D1 id, the R2 bucket, the
Access team and AUD, and the owner (secret `PERSONAL_OWNER_ID`). A placeholder fails closed: requests are refused.

## Commands

```powershell
pnpm install
pnpm test
pnpm db:generate   # compare with drizzle/0000 before accepting new output
```
