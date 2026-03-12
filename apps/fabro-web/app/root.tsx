import {
  isRouteErrorResponse,
  Links,
  Meta,
  Outlet,
  Scripts,
  ScrollRestoration,
} from "react-router";

import type { Route } from "./+types/root";
import { ThemeProvider } from "./lib/theme";
import "./app.css";

const themeScript = `(function(){try{var t=localStorage.getItem("fabro-theme");if(t!=="light"&&t!=="dark")t=window.matchMedia("(prefers-color-scheme:dark)").matches?"dark":"light";document.documentElement.classList.add(t)}catch(e){document.documentElement.classList.add("dark")}})()`;

export const links: Route.LinksFunction = () => [
  { rel: "icon", href: "/favicon.svg", type: "image/svg+xml" },
  { rel: "icon", href: "/favicon.ico", sizes: "32x32" },
  { rel: "apple-touch-icon", href: "/apple-touch-icon.png" },
  { rel: "preconnect", href: "https://fonts.googleapis.com" },
  {
    rel: "preconnect",
    href: "https://fonts.gstatic.com",
    crossOrigin: "anonymous",
  },
  {
    rel: "stylesheet",
    href: "https://fonts.googleapis.com/css2?family=Geist:wght@100..900&family=JetBrains+Mono:wght@400;500;600&display=swap",
  },
];

export function Layout({ children }: { children: React.ReactNode }) {
  return (
    <html lang="en" className="h-full bg-atmosphere" suppressHydrationWarning>
      <head>
        <script dangerouslySetInnerHTML={{ __html: themeScript }} />
        <meta charSet="utf-8" />
        <meta name="viewport" content="width=device-width, initial-scale=1" />
        <Meta />
        <Links />
      </head>
      <body className="h-full">
        {children}
        <ScrollRestoration />
        <Scripts />
      </body>
    </html>
  );
}

export default function App() {
  return (
    <ThemeProvider>
      <Outlet />
    </ThemeProvider>
  );
}

export function ErrorBoundary({ error }: Route.ErrorBoundaryProps) {
  let status = 500;
  let heading = "Something went wrong";
  let message = "An unexpected error occurred. Please try again.";
  let stack: string | undefined;

  if (isRouteErrorResponse(error)) {
    status = error.status;
    if (status === 404) {
      heading = "Page not found";
      message = "The page you're looking for doesn't exist or has been moved.";
    } else {
      heading = `${status} Error`;
      message = error.statusText || message;
    }
  } else if (error instanceof Error) {
    if (import.meta.env.DEV) {
      message = error.message;
      stack = error.stack;
    }
  }

  const is404 = status === 404;

  return (
    <main className="flex min-h-full flex-col items-center justify-center px-6 py-24">
      <div className="text-center">
        <p
          className="font-mono text-[8rem] font-bold leading-none tracking-tighter"
          style={{
            background: is404
              ? "linear-gradient(180deg, var(--color-teal-500) 0%, var(--color-teal-700) 100%)"
              : "linear-gradient(180deg, var(--color-coral) 0%, #a63e3e 100%)",
            WebkitBackgroundClip: "text",
            WebkitTextFillColor: "transparent",
            opacity: 0.8,
          }}
        >
          {status}
        </p>

        <h1 className="mt-4 text-2xl font-semibold tracking-tight text-fg">
          {heading}
        </h1>
        <p className="mt-2 max-w-md text-sm leading-relaxed text-fg-3">
          {message}
        </p>

        <div className="mt-8 flex items-center justify-center gap-3">
          <a
            href="/runs"
            className="inline-flex items-center gap-2 rounded-md border border-teal-500/20 bg-teal-500/10 px-4 py-2 text-sm font-medium text-teal-500 transition-colors hover:border-teal-500/40 hover:bg-teal-500/15 hover:text-fg"
          >
            Go to Runs
          </a>
          <button
            type="button"
            onClick={() => window.history.back()}
            className="inline-flex items-center gap-2 rounded-md border border-line px-4 py-2 text-sm font-medium text-fg-3 transition-colors hover:border-line-strong hover:bg-overlay hover:text-fg"
          >
            Go back
          </button>
        </div>
      </div>

      {stack && (
        <details className="mt-12 w-full max-w-3xl">
          <summary className="cursor-pointer text-xs font-medium text-fg-muted transition-colors hover:text-fg-3">
            Stack trace
          </summary>
          <pre className="mt-2 max-h-64 overflow-auto rounded-lg border border-line bg-panel/60 p-4 font-mono text-xs leading-relaxed text-fg-3">
            <code>{stack}</code>
          </pre>
        </details>
      )}
    </main>
  );
}
