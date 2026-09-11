# DroneOpsCommand

**Self-hosted mission management, flight log analysis, GPS flight replay with video export, AI report generation, invoicing, and real-time airspace monitoring for commercial drone operators.**

**Version 2.90.0** | [Quick Start](#quick-start) | [Features](#features) | [Configuration](#configuration) | [Contributing](CONTRIBUTING.md) | [License](LICENSE)

**Live Demo:** [command-demo.barnardhq.com](https://command-demo.barnardhq.com) (login: `demo` / `demo123`)

---

DroneOpsCommand is a self-hosted, full-stack platform for managing commercial drone operations end-to-end. It covers the complete lifecycle from flight data ingestion and GPS telemetry visualization through AI-powered report generation, invoicing, and client delivery — all running on your own hardware.

Designed for FAA Part 107 certified operators running missions such as search & rescue, inspections, mapping, videography, and more.

### Why DroneOpsCommand?

- **100% self-hosted** — runs on your own hardware via Docker Compose. No cloud dependencies, no per-seat licensing, no subscription fees.
- **AI report generation** — local via Ollama (Qwen 2.5 3B default) or cloud via Claude API. Your data stays on your hardware with Ollama; Claude API available for faster, higher-quality output.
- **White-label ready** — company name, tagline, and branding are fully configurable from the Settings UI. No code changes needed to make it yours.
- **Full lifecycle** — flight log upload, GPS path visualization, animated flight replay with video export, telemetry analysis, mission management, AI reports, PDF export, invoicing, and email delivery in one platform.
- **Real-time airspace** — live aircraft tracking via OpenSky Network with anonymous or authenticated access.
- **Mobile-friendly** — responsive dark-themed UI works on phones, tablets, and desktops.

---

## Table of Contents

- [Quick Start](#quick-start)
- [Features](#features)
- [Architecture](#architecture)
- [Configuration](#configuration)
- [Updating](#updating)
- [Pages & Workflows](#pages--workflows)
- [Backend Services](#backend-services)
- [API Reference](#api-reference)
- [Roadmap](#roadmap)
- [Development](#development)
- [Contributing](#contributing)
- [License](#license)

---

## Quick Start

### Prerequisites

- [Docker](https://docs.docker.com/get-docker/) and [Docker Compose](https://docs.docker.com/compose/install/) (v2+)
- x86_64 or ARM64 host

**Minimum resources (self-hosted):**

| Resource | Minimum | Recommended | Notes |
|----------|---------|-------------|-------|
| RAM      | 8 GB    | 16 GB       | Ollama alone reserves ~4 GB for the quantized model; backend + Postgres + flight-parser + Redis under load push past 8 GB total. |
| CPU      | 4 cores | 6–8 cores   | `docker-compose.yml` pins Ollama to 6 cores. Fewer cores means slow AI report generation. |
| Disk     | 30 GB   | 100 GB+     | Flight logs, Postgres, Ollama model, video exports. Grows with usage. |

> **Docker Desktop users (Windows/Mac) — READ THIS.** Docker Desktop runs containers inside a Linux VM with its own RAM/CPU limits. The defaults are usually **too low** for DroneOpsCommand. Open **Docker Desktop → Settings → Resources** and raise **Memory to at least 8 GB** (16 GB recommended) and **CPUs to at least 4** before `docker compose up`. If the VM runs out of memory the stack will crash at startup or under load with no clear error. `setup-server.sh` does not run on Windows/Mac, so you won't see a preflight warning — allocate the VM resources manually.

> **Windows?** See the [Windows Self-Hosting Guide](docs/windows-self-hosting.md) for step-by-step Docker Desktop + WSL 2 setup.

### Setup

```bash
# 1. Clone and configure
git clone https://github.com/BigBill1418/DroneOpsCommand.git
cd DroneOpsCommand
cp .env.example .env

# 2. Set your secrets (IMPORTANT: change these before first run)
#    Edit .env and update at minimum:
#    - POSTGRES_PASSWORD
#    - JWT_SECRET_KEY

# 3. Launch
docker compose up -d

# 4. Wait for the AI model to download (first run only, ~1.5GB)
docker compose logs -f ollama-setup

# 5. Open the app
#    Web UI:   http://localhost:3080
#    API docs: http://localhost:3080/docs
#    First visit shows the setup wizard — create your admin account there.
```

### What happens on first startup

1. PostgreSQL schema is created automatically
2. Setup wizard prompts you to create the admin account (no env vars needed)
3. Aircraft fleet (6 DJI models) and rate templates (8 billing presets) are pre-loaded
4. Ollama downloads the Qwen 2.5 3B model (~1.5GB)
5. All storage directories are created

### Auto-start on boot

Run the setup script to install the boot-time systemd unit:

```bash
sudo ./setup-server.sh             # install droneops.service
sudo ./setup-server.sh --uninstall # remove it (also cleans up legacy autopull units)
```

This installs **one** systemd unit:
- **`droneops.service`** — starts the Docker Compose stack on boot

```bash
# Useful commands
systemctl status droneops          # stack status
journalctl -u droneops -f          # stack start/stop logs
docker compose logs -f backend     # application logs
```

### Updating

There is **no in-repo deploy script** — `update.sh` and the per-repo
`autopull` poller were both removed (see
[ADR-0018](docs/adr/0018-deploy-path-is-noc-fleet-deployer.md)); having two
deploy paths where one was broken actively misled operators. To update a
self-hosted install:

```bash
git pull --ff-only
docker compose up -d --build       # rebuild only what changed
docker compose ps                  # confirm everything is healthy
```

Database migrations run automatically on backend startup, so no separate
migrate step is needed.

> On BarnardHQ's own production host, deploys are handled by an external
> fleet deployer that polls `origin/main` — not by anything in this repo.
> Self-hosters use the two commands above.

All containers have healthchecks and `restart: unless-stopped`, so individual services auto-recover from crashes. The backend retries DB and Redis connections on startup to handle restart race conditions.

### Personalize it

After logging in, go to **Settings > Branding** to set your company name, tagline, website, and contact email. These appear on PDF reports, emails, the login page, and customer-facing pages.

---

## Features

### Mission Management
- Create and track drone missions across 9 mission types: Search & Rescue, Videography, Lost Pet Recovery, Inspection, Mapping, Photography, Survey, Security & Investigations, Other
- Multi-step mission wizard: Details, Flights, Images, Report, Invoice
- Mission status tracking: Draft, Completed, Sent
- Billable/non-billable designation per mission
- Edit existing missions at any step

### Flight Log Upload & Parsing
- Drag-and-drop upload of DJI flight logs (.txt, .csv, .dat, .log)
- Folder upload support — drop an entire SD card directory and all valid logs are extracted
- Automatic batched uploads (40MB per batch) for large log sets — no more 413 errors
- Dedicated flight-parser microservice for DJI TXT log decryption via DJI API
- SHA-256 deduplication — re-uploading the same log is silently skipped
- Original log files stored on disk for future re-processing
- Manual flight entry for non-DJI aircraft

### Flight Data & Statistics
- Aggregate flight statistics: total flights, total time, total distance, max altitude, max speed
- Per-drone breakdown of flight time (visual bar chart)
- Top flights by duration and distance
- Searchable, sortable flight log table with unit conversions (meters to feet/miles, m/s to mph)
- Average distance and duration per flight
- Flight detail drawer with interactive GPS flight path map over dark CartoDB tiles
- Green takeoff marker, orange landing marker, cyan flight path trace
- Export individual flights as GPX, KML, or CSV

### Telemetry Visualization
- Dedicated telemetry page with time-series charts
- Altitude, speed, battery percentage, voltage, temperature, satellite count, signal strength, distance from home
- Auto-downsampled to 2,000 points for smooth rendering of large datasets
- Per-flight telemetry accessible from the flight detail view

### Multi-Flight Path Maps
- Interactive Leaflet map with dark CartoDB basemap matching the app's dark theme
- Color-coded flight path overlays with start/end point markers
- Convex hull polygon showing total coverage area boundary
- Coverage area calculation in acres (with 30m buffer for camera swath simulation)
- Static map PNG generation for PDF embedding
- UTM coordinate conversion for accurate area measurement via Shapely/PyProj

### Real-Time Airspace Monitoring
- Live aircraft positions via OpenSky Network API
- Works anonymously (no account needed) with optional authenticated mode for higher rate limits
- Configurable search radius in nautical miles around your location
- Aircraft callsign, altitude, speed, heading, vertical rate, squawk code, and ground status
- Auto-refresh with configurable interval

### OpenDroneLog Integration
- Pull flight logs from your self-hosted OpenDroneLog instance
- Select and attach specific flights to each mission
- GPS track extraction with telemetry data (altitude, speed, distance, duration)
- Automatic data normalization across OpenDroneLog API versions
- Connection testing from Settings page

### AI Report Generation
- Dual LLM provider support: local via Ollama or cloud via Claude API (Anthropic)
- Ollama default model: Qwen 2.5 3B — runs on your hardware, data stays local
- Claude API option: Claude Sonnet for faster, higher-quality reports (requires API key)
- Switchable from Settings page — choose provider per deployment
- Operator enters field notes/narrative, LLM generates professional after-action report
- Structured report sections: Mission Overview, Area Coverage, Flight Operations Summary, Key Findings, Recommendations
- Async generation via Celery worker with status polling
- Editable output — review and modify the generated report before finalizing
- LLM status monitoring on Settings page (online/offline, loaded model, active provider)
- Configurable model, temperature, and token limits

### Rich Text Editing
- TipTap-powered WYSIWYG editor for report narratives
- Full formatting: bold, italic, underline, strikethrough, headings, lists, blockquotes, code, alignment, links
- Edit both operator narrative and final report content

### Aircraft Fleet Management
- Pre-seeded DJI aircraft profiles: Matrice 30T, Matrice 4TD, Mavic 3 Pro, Avata 2, FPV, Mini 5 Pro
- Detailed specifications per aircraft: flight time, max speed, camera, thermal imaging, sensors, weight, transmission range
- Add/edit/delete aircraft with custom specs (stored as JSON)
- Assign aircraft to individual flights within a mission
- Aircraft cards displayed in mission detail and PDF reports

### PDF Export
- Branded PDF reports with custom company branding via WeasyPrint
- Includes: mission metadata, report narrative, flight map, aircraft specs, mission images with captions
- Invoice section with line items, totals, tax calculation
- Payment links (PayPal/Venmo) for unpaid invoices
- Optional client download link for mission footage — **withheld until the
  invoice is paid in full** (ADR-0039; per-report operator override available)
- Generated timestamp and mission ID

### Invoicing
- Per-mission invoicing with automatic duplicate prevention
- Line items with 6 categories: Billed Time, Travel, Rapid Deployment, Equipment, Special Circumstances, Other
- Quantity, unit price, and calculated line totals
- Automatic subtotal, configurable tax rate, tax amount, and grand total
- Paid-in-full tracking
- Rate templates for quick line item creation (configurable in Settings)
- Sort ordering for line item display

### Rate Templates
- Reusable billing templates: Standard Hourly Rate, Mileage, Flat Rate Travel, Rapid Deployment, Night Operations Surcharge, Thermal Imaging, Video Editing, Report Preparation
- Configurable default quantity, unit (hours, miles, flat, each), and rate
- Active/inactive toggle
- Add/edit/delete from Settings page

### Pilot Management
- Pilot profiles with name, FAA certificate number, and certifications
- Per-pilot flight hour tracking and summary
- Assign pilots to flights for regulatory compliance
- Managed from the Settings page

### Battery Management
- Track individual batteries by serial number and custom name
- Cycle count, health status, and usage history
- Link batteries to flights for lifecycle tracking

### Maintenance Tracking
- Schedule and log maintenance records for each aircraft
- Customizable maintenance types (no fixed-length limits)
- Attach photos to maintenance records (up to 10MB per image)
- Track completion dates and upcoming maintenance due dates

### Backup & Restore
- Full database backup export from the UI
- Upload and restore backups to recover or migrate data
- Validate backup files before restoring

### Customer CRM & Intake
- Customer profiles: name, company, email, phone, address (including city, state, zip), notes
- Address auto-complete via OpenStreetMap/Nominatim geocoding
- Search across all customer fields
- Customer linked to missions for reporting and email delivery
- Job history tracking
- Digital customer intake with tokenized links and expiration
- Terms of Service signature capture with PDF storage
- Configurable default TOS document upload

### Device API Keys
- Generate API keys for field devices (DroneOpsSync companion app)
- Device-authenticated upload endpoint for automated flight log sync from remote controllers
- Key management (create, revoke) from the Settings page

### Email Delivery
- Send PDF reports directly to customer email
- Async SMTP with TLS support via aiosmtplib
- HTML email body with mission title and optional download link (payment-gated per ADR-0039)
- Automated follow-up email delivers the download link the moment the
  invoice is paid in full — Stripe or manual mark-paid (ADR-0040)
- PDF attached automatically
- SMTP configuration via Settings page with test email button
- Mission status updated to "Sent" after successful delivery

### UNAS NAS Integration
- Store mission footage folder paths (UNAS/Synology NAS)
- Paste share links from UNAS web interface with expiration dates
- Active/expired link status badge with date tracking
- Optional inclusion of download link in client reports and emails —
  payment-gated: withheld until the invoice is paid in full, then delivered
  automatically by email and unlocked in the client portal (ADR-0039/0040)
- Supports file paths with special characters, unicode, and spaces

### Weather & Airspace Intelligence
- Real-time weather conditions via Open-Meteo API: temperature, humidity, wind speed/direction/gusts, cloud cover, visibility, pressure
- METAR aviation weather from AviationWeather.gov: flight category (VFR/MVFR/IFR/LIFR), raw METAR string, cloud layers
- FAA Temporary Flight Restrictions (TFRs) from AviationWeather.gov/FAA GeoJSON
- NOTAMs (Notices to Airmen) with classification and effective dates
- National Weather Service alerts with severity levels
- Wind severity indicator: favorable, caution, hazardous
- Configurable home location from Settings page (used for weather, airspace, and METAR data)

### Financial Dashboard
- Total billed revenue, average per mission, billable mission count
- Revenue breakdown by: drone/aircraft, line item category, mission type, month, customer
- Top customers by revenue with mission count
- Paid vs. outstanding tracking and collection rate percentage
- Searchable invoice table across all missions
- Prepaid status badges

### Flight Replay
- Animated GPS flight path playback with real-time telemetry HUD
- Altitude-colored trail segments (ground, <100ft, 100-200ft, 200-400ft, 400ft+)
- Animated drone marker with heading rotation and glow effect
- Ghost trail showing full flight path with colored trail progressing over it
- Playback controls: play/pause (spacebar), skip forward/back, scrub bar
- Variable speed: 0.5x, 1x, 2x, 5x, 10x
- Live telemetry sidebar: altitude, speed, heading, position, elapsed time
- Flight stats panel with duration, distance, max altitude, max speed
- Home point and start/end markers on map
- Follow-drone mode auto-pans the map to track the aircraft
- Dark CartoDB basemap matching the app's theme

### Flight Video Export
- One-click export: click button → render → auto-download
- Renders full flight replay as a downloadable WebM video (1920x1080, 30fps)
- Canvas-based rendering with CartoDB dark map tiles, altitude-colored trail, drone marker
- Telemetry sidebar overlay with live altitude, speed, heading, position, elapsed time
- Flight stats panel, altitude color legend, and progress bar in the video
- Progress notifications during rendering with percentage updates
- No modal or multi-step flow — instant click-to-download behavior
- Browser-native MediaRecorder with VP9/VP8 codec support
- Ideal for after-action reports and customer deliverables

### Client Portal
- Client-facing view of their missions and invoices — no operator internals exposed
- Signed JWT links emailed to clients with configurable expiry — no account creation needed
- Optional password-protected persistent login for repeat clients
- Mission status visibility: Scheduled, In Progress, Processing, Review, Delivered
- Client views and pays invoices via Stripe (card/ACH) directly in the portal
- Stripe webhook automatically marks invoices paid in the operator's financial dashboard
- Operator generates client access links from the mission detail page

### Authentication & Security
- JWT access tokens (configurable expiration, default 30 min)
- Refresh token rotation (configurable, default 30 days)
- Secure password hashing via bcrypt 4.x (direct, no passlib wrapper)
- All API endpoints require authentication
- Admin account seeded on first startup only — password never overwritten on restart
- PostgreSQL advisory lock prevents race conditions during seed
- Single-worker uvicorn for consistent async behavior
- Explicit commit + read-back verification on password changes

### Dashboard
- At-a-glance stats: total flight hours, total flights, total missions, drafts, customers
- Recent missions table with status badges and quick actions
- Live weather conditions and flight conditions assessment
- METAR aviation weather with color-coded flight categories (VFR/MVFR/IFR/LIFR)
- FAA TFR and NOTAM alerts
- NWS weather alerts with severity levels
- Dark-themed UI with cyan accents, Bebas Neue headings, and Share Tech Mono data fonts

### Image Management
- Upload mission images with drag-and-drop or file picker
- Automatic image resizing (max 1920px) for report optimization
- EXIF orientation correction
- JPEG conversion for consistency and file size reduction
- Captions per image
- Sort ordering for report display
- Images embedded in PDF reports

---

## Architecture

| Service | Technology | Port | Purpose |
|---------|-----------|------|---------|
| Frontend | React 18 + Vite + Mantine UI | 3080 (nginx) | SPA web interface |
| Backend | Python 3.12, FastAPI, SQLAlchemy 2.0 | 8000 | REST API |
| Database | PostgreSQL 16 Alpine | 5434:5432 | Persistent storage with replication support |
| Flight Parser | Python microservice | 8100 | DJI flight log decryption and parsing |
| LLM | Ollama (Qwen 2.5 3B default) or Claude API | 11434 | AI report generation |
| Queue | Redis 7 Alpine | 6379 | Celery task broker |
| Worker | Celery (same backend image) | — | Async report generation |
| Watchtower | containrrr/watchtower | — | Base image auto-update |
| Cloudflared | cloudflare/cloudflared | — | Secure tunnel (optional) |

### PostgreSQL Streaming Replication

The primary database is configured for WAL streaming replication to a standby on CHAD-HQ (10.99.0.2). This provides:

- **Hot standby** — read-only replica available for failover
- **Continuous WAL shipping** — changes stream in real-time to the standby
- **Replication user** — dedicated `replicator` role with `REPLICATION` privileges
- **Managed by NOC** — replication health monitored by NOC Master's continuous replication monitor (30s checks, auto-recovery)

The primary entrypoint script (`scripts/primary-entrypoint.sh`) configures `pg_hba.conf` for replication access and WAL sender settings. The standby configuration is in `docker-compose.standby.yml`.

### Frontend Stack
- **React 18** with TypeScript
- **Mantine UI v7** component library with dark theme
- **Vite** build tool
- **React Router** for SPA navigation
- **TipTap** rich text editor
- **Leaflet** interactive maps with dark CartoDB basemap
- **Axios** HTTP client with JWT interceptors and token refresh
- **Mantine Dates** for date inputs
- **Tabler Icons** icon set
- Custom fonts: Bebas Neue (display), Share Tech Mono (monospace), Rajdhani (UI)

### Backend Stack
- **FastAPI** async REST framework
- **SQLAlchemy 2.0** async ORM with asyncpg driver
- **Pydantic v2** request/response validation
- **WeasyPrint** HTML-to-PDF rendering (Cairo/Pango)
- **Jinja2** HTML templates for PDF and email
- **Shapely + PyProj** geospatial calculations
- **staticmap** static map image generation
- **Pillow** image processing
- **aiosmtplib** async email delivery
- **httpx** async HTTP client (Ollama, OpenDroneLog, weather APIs)
- **Celery** distributed task queue
- **bcrypt** direct password hashing (4.x)

### Nginx Reverse Proxy
- Proxies `/api/` to backend on port 8000
- Proxies `/static/` and `/uploads/` to backend
- SPA fallback (`try_files` to `index.html`)
- Gzip compression enabled
- 200MB max upload size, 300s read timeout

---

## Configuration

All settings are configured via environment variables in the `.env` file.

### Database
| Variable | Default | Description |
|----------|---------|-------------|
| `POSTGRES_USER` | `doc` | PostgreSQL username |
| `POSTGRES_PASSWORD` | `changeme_in_production` | PostgreSQL password |
| `POSTGRES_DB` | `doc` | Database name |
| `DATABASE_URL` | `postgresql+asyncpg://...` | Full async connection string |

### Authentication
| Variable | Default | Description |
|----------|---------|-------------|
| `JWT_SECRET_KEY` | `changeme_generate_a_random_secret` | **Change this** — used to sign tokens |
| `JWT_ALGORITHM` | `HS256` | Token signing algorithm |
| `JWT_ACCESS_TOKEN_EXPIRE_MINUTES` | `30` | Access token lifetime |
| `JWT_REFRESH_TOKEN_EXPIRE_DAYS` | `30` | Refresh token lifetime |

> **Note:** Admin credentials are created via the first-run setup wizard in the browser. There are no `ADMIN_USERNAME`/`ADMIN_PASSWORD` environment variables.

### Integrations
| Variable | Default | Description |
|----------|---------|-------------|
| `OPENDRONELOG_URL` | *(empty)* | Your OpenDroneLog server URL (e.g., `http://192.168.1.50:8080`) |
| `DJI_API_KEY` | *(empty)* | DJI Cloud API key for encrypted flight log parsing ([register here](https://developer.dji.com)) |
| `OLLAMA_BASE_URL` | `http://ollama:11434` | Ollama API endpoint |
| `OLLAMA_MODEL` | `qwen2.5:3b` | Ollama model for report generation |
| `LLM_PROVIDER` | `ollama` | Active LLM provider: `ollama` or `claude` |
| `ANTHROPIC_API_KEY` | *(empty)* | Anthropic API key (required when `LLM_PROVIDER=claude`) |

### Email (SMTP)
| Variable | Default | Description |
|----------|---------|-------------|
| `SMTP_HOST` | *(empty)* | SMTP server hostname |
| `SMTP_PORT` | `587` | SMTP port |
| `SMTP_USER` | *(empty)* | SMTP username |
| `SMTP_PASSWORD` | *(empty)* | SMTP password |
| `SMTP_FROM_EMAIL` | *(empty)* | Sender email address |
| `SMTP_FROM_NAME` | *(empty)* | Sender display name |
| `SMTP_USE_TLS` | `true` | Enable TLS encryption |

SMTP settings can also be configured from the Settings page in the web UI (stored in database, overrides env vars).

### Stripe (Client Portal Payments)
| Variable | Default | Description |
|----------|---------|-------------|
| `STRIPE_SECRET_KEY` | *(empty)* | Stripe secret key for payment processing |
| `STRIPE_PUBLISHABLE_KEY` | *(empty)* | Stripe publishable key (exposed to frontend) |
| `STRIPE_WEBHOOK_SECRET` | *(empty)* | Stripe webhook signing secret |

### Networking
| Variable | Default | Description |
|----------|---------|-------------|
| `FRONTEND_URL` | `http://localhost:3080` | Public URL used in intake emails and client portal links |
| `FRONTEND_PORT` | `3080` | Host port for the frontend. Use `127.0.0.1:3080` to restrict to localhost when behind a tunnel |
| `CLOUDFLARE_TUNNEL_TOKEN` | *(empty)* | Cloudflare Tunnel token for secure remote access without opening ports |

### Watchtower (Auto-Update)
| Variable | Default | Description |
|----------|---------|-------------|
| `WATCHTOWER_MONITOR_ONLY` | `false` | Set to `true` to get notifications only (no auto-update) |
| `WATCHTOWER_NOTIFICATION_URL` | *(empty)* | Shoutrrr notification URL (Slack, Discord, email, etc.) |

### Replication
| Variable | Default | Description |
|----------|---------|-------------|
| `REPLICATION_PASSWORD` | *(no default — required)* | Password for the PostgreSQL replication user. Set in your `.env`; per ADR-0012 there is no fallback default and containers will refuse to start if this is unset. |

### Storage
| Variable | Default | Description |
|----------|---------|-------------|
| `UPLOAD_DIR` | `/data/uploads` | Mission image storage path |
| `REPORTS_DIR` | `/data/reports` | Generated PDFs and map images |

### LLM Provider Selection
Two LLM providers are supported, configurable from the Settings page or via environment variables:

| Provider | Model | Where it runs | When to use |
|----------|-------|---------------|-------------|
| **Ollama** (default) | Qwen 2.5 3B | Local, on your hardware | Data stays on-premises, no API costs |
| **Claude API** | Claude Sonnet | Anthropic cloud | Faster, higher-quality reports, requires API key |

Set `LLM_PROVIDER=claude` and `ANTHROPIC_API_KEY` in `.env` to use Claude, or switch providers in Settings at runtime.

### Ollama Performance Tuning
The `docker-compose.yml` pins Ollama to 6 CPU cores (leaving 2 for the OS and database), sets 8GB RAM reservation, enables flash attention, and keeps the model loaded permanently (`OLLAMA_KEEP_ALIVE=-1`).

---

## Updating

### Updating the application

```bash
git pull --ff-only
docker compose up -d --build
docker compose ps                  # confirm healthy
```

Compose rebuilds only the services whose build context changed, and database
migrations run automatically on backend startup — there is no separate migrate
step.

There is deliberately **no in-repo auto-deploy poller.** An earlier
`autopull.sh` + `droneops-autopull.timer` + `update.sh` arrangement was removed
([ADR-0018](docs/adr/0018-deploy-path-is-noc-fleet-deployer.md)): `autopull.sh`
still invoked the already-deleted `update.sh`, so the repo advertised two deploy
paths while one of them was a corpse. That cost real debugging time. If you want
polling-based CD on a self-hosted install, drive it from outside the repo (a
systemd timer of your own, a CI runner, or a webhook) rather than reintroducing
a second in-repo path.

### Watchtower (base image updates)

Watchtower runs as a sidecar service checking for updated base images (PostgreSQL, Redis, Ollama) daily. By default it auto-updates; set `WATCHTOWER_MONITOR_ONLY=true` to get notifications without auto-updating. Configure `WATCHTOWER_NOTIFICATION_URL` in `.env` for alerts (supports Slack, Discord, email, etc. via [Shoutrrr](https://containrrr.dev/shoutrrr/)).

---

## Pages & Workflows

### Dashboard (`/`)
Landing page with mission stats, recent missions table, real-time weather conditions, METAR aviation data, FAA TFR/NOTAM alerts, NWS weather alerts, and flight condition assessment.

### Missions (`/missions`)
List all missions with status badges (Draft/Completed/Sent), billable indicators, search, and quick actions (edit, delete). Click a row to view mission details.

### New Mission (`/missions/new`)
Multi-step wizard with 5 stages:

1. **Details** — Customer, title, type, date, location, description, billable toggle, UNAS folder path, download link URL with expiration
2. **Flights** — Browse OpenDroneLog flights, select flights for the mission, assign aircraft to each flight, view flight map and coverage area
3. **Images** — Upload mission photos (drag-and-drop or file picker), auto-resized and EXIF-corrected
4. **Report** — Enter operator narrative, generate LLM report, edit in rich text editor, generate PDF, email to customer
5. **Invoice** — Add line items from rate templates or manually, set quantities and rates, configure tax, mark paid/unpaid

### Mission Detail (`/missions/:id`)
Full mission view with metadata, assigned aircraft cards, interactive flight map, coverage stats, download link status, report content, and action buttons (edit, delete, generate PDF, email report).

### Flights (`/flights`)
Flight library with aggregate statistics, per-drone breakdowns, top flights, sortable/searchable table, and a detail drawer with interactive GPS flight path map, telemetry data, and export options (GPX/KML/CSV). Flight Replay button for flights with GPS tracks.

### Flight Replay (`/flights/:id/replay`)
Animated GPS flight path playback with altitude-colored trail, drone marker with heading, live telemetry sidebar (altitude, speed, heading, position), flight stats, and playback controls (play/pause, speed, scrub). One-click video export renders the full replay as a downloadable WebM video with map, flight path, and telemetry overlay.

### Upload Logs (`/upload-logs`)
Drag-and-drop flight log upload with folder support. Batched uploads for large file sets. Progress tracking per file with duplicate detection and error reporting.

### Telemetry (`/telemetry`)
Time-series telemetry visualization with altitude, speed, battery, satellites, signal, and distance-from-home charts. Auto-downsampled for smooth rendering.

### Airspace (`/airspace`)
Live aircraft tracking via OpenSky Network. Works anonymously or with credentials for better rate limits. Configurable location and search radius.

### Batteries (`/batteries`)
Battery fleet management with serial numbers, cycle counts, health tracking, and per-battery flight history.

### Maintenance (`/maintenance`)
Maintenance scheduling and logging per aircraft. Custom maintenance types, photo attachments, and due-date tracking.

### Customers (`/customers`)
Customer CRM with add/edit/delete, address auto-complete via OpenStreetMap geocoding, digital intake forms with TOS signature capture, and search.

### Financials (`/financials`)
Revenue dashboard with total/average/outstanding metrics, breakdowns by drone, category, mission type, month, and customer. Full invoice table with search.

### Setup (`/setup`)
First-run wizard shown when no users exist. Creates the initial admin account. To reset: `docker compose exec backend python reset_to_setup.py`.

### Client Portal (`/client`)
Client-facing mission dashboard and invoice payment. Clients access via signed JWT links sent from the operator. Separate authentication scope from the operator UI.

### Settings (`/settings`)
System configuration across multiple tabs:
- **LLM** — Provider selection (Ollama or Claude API), connection status, loaded model, API key configuration
- **Flight Data** — OpenSky Network credentials, DJI API key, OpenDroneLog server URL with connection test
- **SMTP** — Email server configuration with test email
- **Payment Links** — PayPal and Venmo URLs for invoices
- **Stripe** — Stripe API keys for client portal payments
- **Home Location** — Coordinates for weather, airspace, and METAR data
- **Aircraft Fleet** — Add/edit/delete aircraft with specifications
- **Pilots** — Add/edit/delete pilot profiles with certifications and flight hour tracking
- **Rate Templates** — Add/edit/delete billing rate presets
- **Device Keys** — API key management for DroneOpsSync companion app
- **Backup** — Database export and restore
- **Branding** — Company name, tagline, website, social media, contact email — used in PDF reports, emails, login page, and customer-facing pages

---

## Backend Services

### PDF Generator
Renders branded PDF reports from Jinja2 HTML templates via WeasyPrint. Includes mission metadata, report narrative, flight path map, aircraft specs, mission images, invoice with line items and totals, payment links, and optional download link.

### Email Service
Async SMTP client that sends HTML emails with the PDF report attached. Loads configuration from database settings first, falls back to environment variables. Supports TLS.

### LLM Provider Service
Dual-provider LLM client supporting Ollama and Claude API. The active provider is selectable from Settings or via `LLM_PROVIDER` env var.

- **Ollama** — HTTP client to the `/api/generate` endpoint. Sends a structured prompt with mission data, flight telemetry, and operator notes. Temperature 0.3 for consistency, 300s timeout, 6 CPU threads.
- **Claude API** — Anthropic SDK client using Claude Sonnet. Same structured prompt, cloud-processed. Requires `ANTHROPIC_API_KEY`.

### OpenDroneLog Client
REST client that fetches flight data from a self-hosted OpenDroneLog instance. Handles multiple API endpoint patterns for version compatibility. Normalizes field names between camelCase and snake_case. Extracts GPS tracks for map rendering.

### Map Renderer
Generates GeoJSON FeatureCollections with flight path LineStrings, start/end markers, and convex hull polygons. Calculates coverage area in acres using UTM projection and Shapely geometry with configurable buffer distance. Renders static PNG maps using OpenStreetMap tiles.

### Flight Parser Service
Standalone microservice that decrypts and parses DJI flight logs using the DJI Cloud API. Extracts GPS tracks, telemetry time-series, drone metadata, and battery information from encrypted TXT log files.

### Airspace Service
Proxies requests to the OpenSky Network API for real-time aircraft position data. Supports anonymous and OAuth2-authenticated modes. Converts search radius to bounding box coordinates and normalizes the response into a clean aircraft list.

### Weather Service
Aggregates data from 4 external APIs: Open-Meteo (current conditions), AviationWeather.gov (METAR, TFRs), aviationapi.com (NOTAMs fallback), and NWS (weather alerts). Location configurable from Settings.

---

## API Reference

Full interactive API documentation is available at `http://localhost:3080/docs` (Swagger UI) when the app is running.

### Core Endpoints

| Method | Endpoint | Description |
|--------|----------|-------------|
| POST | `/api/auth/login` | Authenticate and receive JWT tokens |
| POST | `/api/auth/refresh` | Refresh access token |
| GET/POST | `/api/missions` | List or create missions |
| GET/PUT/DELETE | `/api/missions/{id}` | Read, update, or delete a mission |
| POST | `/api/missions/{id}/flights` | Attach a flight to a mission |
| POST | `/api/missions/{id}/images` | Upload a mission image |
| GET/POST | `/api/missions/{id}/report/generate` | Generate LLM report (async) |
| GET | `/api/missions/{id}/report/status/{task_id}` | Poll report generation status |
| PUT | `/api/missions/{id}/report` | Save/update report content |
| POST | `/api/missions/{id}/report/pdf` | Generate and download PDF |
| POST | `/api/missions/{id}/report/send` | Email PDF report to customer |
| GET/POST | `/api/missions/{id}/invoice` | Get or create invoice |
| POST | `/api/missions/{id}/invoice/items` | Add line item to invoice |
| GET | `/api/missions/{id}/map` | Get flight path GeoJSON |
| GET | `/api/missions/{id}/map/coverage` | Get coverage area in acres |
| POST | `/api/missions/{id}/map/render` | Generate static map PNG |
| GET/POST/PUT/DELETE | `/api/customers` | Customer CRUD |
| GET/POST/PUT/DELETE | `/api/aircraft` | Aircraft CRUD |
| GET | `/api/flight-library` | List all flights in the library |
| GET | `/api/flight-library/{id}` | Flight detail with GPS track and telemetry |
| GET | `/api/flight-library/{id}/track` | Raw GPS track points |
| GET | `/api/flight-library/{id}/telemetry` | Downsampled telemetry time-series |
| GET | `/api/flight-library/{id}/export/{format}` | Export flight as GPX, KML, or CSV |
| POST | `/api/flight-library/upload` | Upload flight log files (batched) |
| POST | `/api/flight-library/device-upload` | Device-authenticated log upload |
| POST | `/api/flight-library/manual` | Create a manual flight entry |
| POST | `/api/flight-library/reprocess/all` | Re-parse stored flight logs |
| GET | `/api/flight-library/airspace/aircraft` | Live aircraft positions (OpenSky proxy) |
| GET/POST/PUT/DELETE | `/api/batteries` | Battery CRUD |
| GET/POST/PUT/DELETE | `/api/maintenance` | Maintenance record CRUD |
| GET/POST | `/api/device-keys` | Device API key management |
| POST | `/api/backup/validate-upload` | Validate a backup file |
| POST | `/api/backup/restore-from-upload` | Restore database from backup |
| GET | `/api/flights` | List flights from OpenDroneLog |
| GET | `/api/financials/summary` | Financial dashboard data |
| GET | `/api/weather/current` | Weather, METAR, TFRs, NOTAMs, NWS alerts |
| GET/PUT | `/api/settings/smtp` | SMTP configuration |
| POST | `/api/settings/smtp/test` | Send test email |
| GET/PUT | `/api/settings/opendronelog` | OpenDroneLog URL |
| GET/PUT | `/api/settings/payment` | PayPal/Venmo payment links |
| GET/POST/PUT/DELETE | `/api/rate-templates` | Rate template CRUD |
| GET | `/api/llm/status` | Ollama/LLM connection status |
| GET/POST | `/api/pilots` | Pilot CRUD |
| GET | `/api/pilots/{id}/hours-summary` | Pilot flight hour breakdown |
| POST | `/api/client/auth/validate` | Validate client portal token |
| POST | `/api/client/auth/login` | Client portal login |
| GET | `/api/client/missions` | Client's mission list |
| GET | `/api/client/missions/{id}` | Client mission detail |
| GET | `/api/client/missions/{id}/invoice` | Client invoice view |
| POST | `/api/client/missions/{id}/invoice/pay` | Initiate Stripe payment |
| POST | `/api/missions/{id}/client-link` | Generate client access link |
| POST | `/api/missions/{id}/client-link/send` | Email client access link |
| POST | `/api/webhooks/stripe` | Stripe payment webhook |
| GET | `/api/auth/setup-status` | Check if initial setup is needed |
| POST | `/api/auth/setup` | Create initial admin account |
| GET | `/api/health` | Health check |

---

## Roadmap

### Next — Multi-Tenant Managed Hosting

Transform DroneOpsCommand from a self-hosted tool into a revenue-generating SaaS product. The self-hosted open-source path remains for operators who want it — managed hosting is the commercial tier.

**Tenant Architecture**
- Schema-per-tenant isolation in PostgreSQL. Each tenant gets their own schema with identical table structures.
- Tenant provisioning API: signup → subdomain claim → payment → automatic schema creation and admin user seeding.
- JWT tenant claims with schema routing middleware. Every authenticated request resolves to the correct tenant schema.
- CI test coverage verifying tenant A cannot read tenant B's data under any code path.

**Billing & Licensing**
- Stripe subscription integration: plan tiers with mission limits, user seat limits, and storage quotas.
- Stripe Checkout hosted page for signup — no custom payment frontend required.
- Stripe Customer Portal for self-service plan changes, payment method updates, and invoice history.
- Webhook-driven plan enforcement: upgrade/downgrade/cancel reflected in tenant configuration automatically.
- Stripe Tax add-on for US sales tax compliance.

**AI Processing (Cloud Offload)**
- Managed tenants use cloud LLM (Anthropic or OpenAI API) instead of local Ollama.
- Per-tenant API key configuration with model selection.
- Celery queue hardening: retry logic, dead letter queue, per-tenant job isolation, queue depth monitoring.
- SLA-aware job priority to prevent report generation backlog across tenants.

**Infrastructure**
- Application servers behind a load balancer with subdomain-based tenant routing.
- Managed PostgreSQL with automated backups.
- S3-compatible object storage for mission images, reports, and deliverables (per-tenant path prefixes).
- Signed URLs for secure file delivery.
- Redis for Celery broker and session caching.

**Pricing Tiers (Planned)**
- Starter — limited missions/month, 1 user, cloud AI reports, email support.
- Professional — higher limits, multi-user with RBAC, priority report generation, UNAS integration.
- Enterprise — unlimited, custom branding, dedicated support, SLA.

---

### Planned — Additional Features

- **Thermal Inspection Report Engine** — Ingest DJI radiometric RJPEG thermal imagery, auto-detect hotspot anomalies via configurable temperature delta thresholds and OpenCV contour detection, annotate visual images with bounding boxes, GPS coordinates, and measured temperatures, and generate branded PDF inspection reports. Plugs into the existing Celery/WeasyPrint report pipeline as a thermal-specific report template. Industry-configurable severity presets (solar/NETA electrical/roofing).
- **Multi-User / Role-Based Access** — Admin and Operator roles at minimum. Foundation for teams and the multi-tenant SaaS tier. Implement before schema changes get harder.
- **Notification System** — Email and in-app alerts for overdue invoices, upcoming maintenance, battery cycle limits, certificate expirations, and completed report generation.
- **Report Template Library** — Multiple PDF templates for different mission types. Inspection reports, SAR after-action reports, videography delivery summaries. Operator-buildable custom templates. LLM prompt adapts per template type.
- **Airspace Pre-Check Workflow** — Drop a pin before mission creation and get a pre-flight airspace assessment: LAANC status, nearby TFRs, Class B/C/D proximity. Uses existing weather/FAA API infrastructure.
- ~~**Claude API Integration**~~ — **Done.** Switchable from Settings or via `LLM_PROVIDER=claude` env var.
- **React Native Android App** — Mission creation, photo capture on-site, report review, and customer lookup from the field. Communicates with the stack via JWT-authenticated HTTPS API.
- **Voice-to-Text** — On-device speech recognition in the Android app for dictating operator field notes hands-free during or after missions.
- **DroneOpsSync Deep Integration** — Field-captured photos auto-upload to the correct mission. Field notes from the controller pre-populate report narrative. JWT API is already in place.
- **Public API & Webhooks** — Let third-party tools (dispatch software, QuickBooks, project management) integrate with Command. Webhooks on mission status changes, invoice payment, and report delivery.
- **Public Demo Instance** — **Live** at [command-demo.barnardhq.com](https://command-demo.barnardhq.com) with pre-loaded sample data, sandboxed operations (demo guard middleware), demo-mode banner with "Deploy Your Own" CTA, and 24-hour auto-reset. See `docker-compose.demo.yml` for deployment config. **Always start the demo via `./bootstrap.sh`** — never `docker compose up -d` directly. The bootstrap script validates `.env.demo` before compose runs, so a missing or incomplete env file produces a clear error instead of silently falling back to default credentials (the failure mode that caused a 6h+ outage on 2026-04-16).

---

## Development

```bash
# Backend
cd backend && pip install -r requirements.txt
uvicorn app.main:app --reload

# Frontend
cd frontend && npm install && npm run dev
```

### Docker Volumes

| Volume | Purpose |
|--------|---------|
| `postgres_data` | PostgreSQL database files |
| `ollama_data` | Downloaded LLM model weights |
| `app_data` | Uploaded images and generated PDFs/maps |

To fully reset the database (destroys all data):
```bash
docker compose down -v
docker compose up -d
```

---

## Contributing

Contributions are welcome! Please see [CONTRIBUTING.md](CONTRIBUTING.md) for guidelines.

---

## License

MIT License — see [LICENSE](LICENSE) for details.
