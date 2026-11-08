import { formatStartedAt, type Session } from "./sessions";

type AppHeaderProps = {
  /** The open session, or undefined on the welcome screen. */
  session: Session | undefined;
  homeDisabled: boolean;
  onHome: () => void;
};

/**
 * Top bar shared by every screen. It doubles as the window's title bar (`titleBarStyle: "Overlay"`
 * in tauri.conf.json): the macOS window controls sit over its left column, and dragging anywhere
 * but its controls moves the window.
 */
export default function AppHeader({ session, homeDisabled, onHome }: AppHeaderProps) {
  return (
    <header
      data-tauri-drag-region="deep"
      className="grid h-16 shrink-0 grid-cols-[1fr_auto_1fr] items-center gap-3 px-2 select-none"
    >
      <button
        type="button"
        className="btn btn-circle btn-lg col-start-2 border-none bg-base-100 shadow-none hover:bg-base-300"
        aria-label="Home"
        aria-current={session ? undefined : "page"}
        disabled={homeDisabled}
        onClick={onHome}
      >
        <HomeIcon filled={!session} />
      </button>
      {session && <h1 className="truncate font-semibold">{formatStartedAt(session)}</h1>}
    </header>
  );
}

/** Filled while the welcome screen is open, outlined elsewhere. */
function HomeIcon({ filled }: { filled: boolean }) {
  return (
    <svg
      viewBox="0 0 24 24"
      className="size-6"
      fill={filled ? "currentColor" : "none"}
      stroke="currentColor"
      strokeWidth={2}
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <path d="M3 10.5 12 3l9 7.5V20a1 1 0 0 1-1 1h-5v-6H9v6H4a1 1 0 0 1-1-1z" />
    </svg>
  );
}
