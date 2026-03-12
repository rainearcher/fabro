import { createSessionStorage } from "react-router";
import crypto from "node:crypto";
import { getDatabase } from "./db.server";

interface SessionRow {
  id: string;
  user_url: string;
  data: string;
  expires_at: number | null;
}

const THIRTY_DAYS_MS = 30 * 24 * 60 * 60 * 1000;

function cleanupExpiredSessions() {
  const db = getDatabase();
  db.prepare("DELETE FROM web_sessions WHERE expires_at IS NOT NULL AND expires_at < ?").run(
    Date.now(),
  );
}

export function createSqliteSessionStorage(secret: string) {
  return createSessionStorage({
    cookie: {
      name: "__fabro_session",
      httpOnly: true,
      sameSite: "lax" as const,
      secure: process.env.NODE_ENV === "production",
      secrets: [secret],
      path: "/",
      maxAge: 30 * 24 * 60 * 60, // 30 days in seconds
    },
    async createData(data, expiresAt) {
      const db = getDatabase();
      const id = crypto.randomUUID();
      const userUrl = (data.userUrl as string) ?? "";
      const expiresAtMs = expiresAt ? expiresAt.getTime() : Date.now() + THIRTY_DAYS_MS;

      db.prepare(
        "INSERT INTO web_sessions (id, user_url, data, expires_at) VALUES (?, ?, ?, ?)",
      ).run(id, userUrl, JSON.stringify(data), expiresAtMs);

      // Probabilistic cleanup (~1% of creates)
      if (Math.random() < 0.01) {
        cleanupExpiredSessions();
      }

      return id;
    },
    async readData(id) {
      const db = getDatabase();
      const row = db
        .prepare("SELECT * FROM web_sessions WHERE id = ?")
        .get(id) as SessionRow | undefined;

      if (!row) return null;

      if (row.expires_at && row.expires_at < Date.now()) {
        db.prepare("DELETE FROM web_sessions WHERE id = ?").run(id);
        return null;
      }

      return JSON.parse(row.data);
    },
    async updateData(id, data, expiresAt) {
      const db = getDatabase();
      const userUrl = (data.userUrl as string) ?? "";
      const expiresAtMs = expiresAt ? expiresAt.getTime() : Date.now() + THIRTY_DAYS_MS;

      db.prepare(
        "UPDATE web_sessions SET user_url = ?, data = ?, expires_at = ? WHERE id = ?",
      ).run(userUrl, JSON.stringify(data), expiresAtMs, id);
    },
    async deleteData(id) {
      const db = getDatabase();
      db.prepare("DELETE FROM web_sessions WHERE id = ?").run(id);
    },
  });
}
