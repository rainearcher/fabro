import Database from "better-sqlite3";
import path from "node:path";
import os from "node:os";
import fs from "node:fs";

let db: Database.Database | null = null;

export function getDatabase(): Database.Database {
  if (db) return db;

  const fabroDir = path.join(os.homedir(), ".arc");
  fs.mkdirSync(fabroDir, { recursive: true });

  db = new Database(path.join(fabroDir, "fabro-web.db"));
  db.pragma("journal_mode = WAL");

  db.exec(`
    CREATE TABLE IF NOT EXISTS web_sessions (
      id TEXT PRIMARY KEY,
      user_url TEXT NOT NULL,
      data TEXT NOT NULL,
      expires_at INTEGER
    );
    CREATE INDEX IF NOT EXISTS idx_web_sessions_user_url ON web_sessions (user_url);
    CREATE INDEX IF NOT EXISTS idx_web_sessions_expires_at ON web_sessions (expires_at);
  `);

  return db;
}
