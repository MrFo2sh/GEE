# Security policy — GAE

- Report issues privately to the maintainer before public disclosure.
- Supported: latest release only.
- This product is defensive tooling. It must never be modified to attack, evade detection, or
  harvest credentials.
- Secrets come only from environment variables listed in `.env.example`.

## What this product exposes

- An HTTP ingest endpoint bound to `127.0.0.1` only (never all interfaces). It refuses any
  request carrying an `Origin` header, so a web page in the user's browser cannot write events.
  Bodies are capped at 4 MB and 10,000 events; the queue to the app is bounded (503 when full).
- File sources read only the files the user names on the command line.
- Event text is displayed, never executed; URLs in artifacts open only when clicked.
- Environments built on GAE must stay defensive: visualize and triage, never attack.
