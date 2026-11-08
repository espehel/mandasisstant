import { formatStartedAt, hasTranscripts, type Session } from "./sessions";

type WelcomeScreenProps = {
  sessions: Session[];
  onStart: () => void;
  onOpen: (sessionId: string) => void;
};

export default function WelcomeScreen({ sessions, onStart, onOpen }: WelcomeScreenProps) {
  const pastSessions = sessions.filter(hasTranscripts);

  return (
    <div className="flex min-h-0 flex-1 gap-2">
      <nav
        aria-labelledby="past-sessions-heading"
        className="flex w-72 shrink-0 flex-col rounded-box bg-base-100"
      >
        <h2
          id="past-sessions-heading"
          className="px-5 pt-6 pb-2 text-sm font-semibold text-base-content/60"
        >
          Past sessions
        </h2>
        {pastSessions.length > 0 ? (
          <ul className="menu w-full flex-1 flex-nowrap overflow-y-auto">
            {pastSessions.map((session) => (
              <li key={session.id}>
                <button
                  type="button"
                  className="flex flex-col items-start gap-0.5"
                  onClick={() => onOpen(session.id)}
                >
                  <span className="font-medium">{formatStartedAt(session)}</span>
                  <span className="w-full truncate text-base-content/60">
                    {session.transcripts[0]}
                  </span>
                </button>
              </li>
            ))}
          </ul>
        ) : (
          <p className="px-5 text-sm text-base-content/60">No sessions yet.</p>
        )}
      </nav>

      <main className="flex flex-1 flex-col items-center justify-center gap-6 rounded-box bg-base-100 p-8">
        <button
          type="button"
          className="btn btn-primary btn-xl h-24 rounded-full px-16 text-3xl shadow-lg"
          onClick={onStart}
        >
          Start session
        </button>
        <p className="max-w-sm text-center text-base-content/60">
          Speak English or Mandarin. Each sentence is transcribed after a short pause.
        </p>
      </main>
    </div>
  );
}
