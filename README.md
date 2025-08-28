# Strava Duplicate Cleaner

> **Note:** This is a Rust rewrite of the original TypeScript implementation: <https://github.com/dylanvanh/strava-bot>

Automatically hides duplicate indoor bike activities on Strava using a scheduled cron job.

## What it does

When you use both a fitness watch and Zwift/MyWhoosh, you get duplicate activities:

- **Indoor bike activity** from your watch (zero distance, unwanted)
- **Virtual ride** from Zwift/MyWhoosh (full workout data)

This bot runs every 15 minutes, finds matching activities within 1 hour of each other, and hides the indoor bike activity while keeping the virtual ride public.

## Setup

### Prerequisites

- Rust toolchain installed
- Strava API application credentials

### Environment Variables

Create a `.env` file:

```env
STRAVA_CLIENT_ID=your_client_id
STRAVA_CLIENT_SECRET=your_client_secret
STRAVA_INITIAL_REFRESH_TOKEN=your_refresh_token
```

### Running

```bash
# Development
cargo run

# Production
cargo run --release
```

The app runs continuously and processes activities every 15 minutes at :00, :15, :30, :45.

## How it works

1. **Fetches recent activities** from Strava API
2. **Identifies indoor bike rides** (type: "Ride", distance: 0)
3. **Finds matching virtual rides** (type: "VirtualRide") within 1 hour
4. **Hides duplicate indoor activities** by setting `hide_from_home: true`
5. **Caches processed activities** to avoid redundant API calls

## Testing

```bash
cargo test
```
