/** Everything transcribed from "Start session" on, across pauses. */
export type Session = {
  id: string;
  /** Milliseconds since the Unix epoch. */
  startedAt: number;
  transcripts: string[];
};

const STORAGE_KEY = "sessions";

const startedAtFormat = new Intl.DateTimeFormat(undefined, {
  dateStyle: "medium",
  timeStyle: "short",
});

export function formatStartedAt(session: Session) {
  return startedAtFormat.format(session.startedAt);
}

export function hasTranscripts(session: Session) {
  return session.transcripts.length > 0;
}

/** Returns the saved sessions, newest first. */
export function loadSessions(): Session[] {
  const stored = localStorage.getItem(STORAGE_KEY);
  if (stored === null) {
    return [];
  }
  // Only saveSessions writes this key.
  const parsed: unknown = JSON.parse(stored);
  return Array.isArray(parsed) ? parsed.filter(isSession) : [];
}

/** Saves the sessions that have transcripts; throws if storage is full. */
export function saveSessions(sessions: Session[]) {
  localStorage.setItem(STORAGE_KEY, JSON.stringify(sessions.filter(hasTranscripts)));
}

function isSession(value: unknown): value is Session {
  return (
    typeof value === "object" &&
    value !== null &&
    "id" in value &&
    typeof value.id === "string" &&
    "startedAt" in value &&
    typeof value.startedAt === "number" &&
    "transcripts" in value &&
    Array.isArray(value.transcripts) &&
    value.transcripts.every((transcript) => typeof transcript === "string")
  );
}
