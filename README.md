# Router Reboot Scheduler

A lightweight Rust application that automatically reboots a mesh router on a configurable schedule using cron format. Designed to run in a Docker container with minimal resource usage.

## Features

- Authenticates with ASUS router using nonce-based authentication
- Reboots a specific mesh node by MAC address
- Configurable schedule using cron format
- Configurable timezone
- Runs in Docker for easy deployment
- Configurable via environment variables
- Timezone-aware scheduling

## Prerequisites

- Docker installed on your local server
- Network access to the router (typically on the same LAN)
- Router admin credentials
- MAC address of the mesh node to reboot

## Configuration

The script uses the following environment variables:

- `ROUTER_IP` - IP address of the router (required, e.g., `192.168.1.1`)
- `ROUTER_USERNAME` - Router admin username (required)
- `ROUTER_PASSWORD` - Router admin password (required)
- `DEVICE_MAC` - MAC address of the mesh node to reboot (required, e.g., `AA:BB:CC:DD:EE:FF`)
- `TIMEZONE` - Timezone for scheduling (default: `UTC`, e.g., `Europe/Warsaw`, `America/New_York`)
- `CRON_SCHEDULE` - Cron expression for reboot schedule (default: `0 3 * * *` = 3:00 AM daily)
- `IMMEDIATE_REBOOT` - Set to `true` to perform an immediate reboot on startup (default: `false`)

### Cron Schedule Format

The `CRON_SCHEDULE` uses standard cron format with 5 fields:
```
minute hour day month day_of_week
```

Examples:
- `0 3 * * *` - Every day at 3:00 AM
- `0 2 * * 0` - Every Sunday at 2:00 AM
- `0 4 * * 1-5` - Every weekday (Monday-Friday) at 4:00 AM
- `30 1 * * *` - Every day at 1:30 AM
- `0 */6 * * *` - Every 6 hours

## Usage

### Using Pre-built Image from GitHub Container Registry

The image is automatically built and published to GitHub Container Registry. You can pull and use it directly:

```bash
# Pull the latest image
docker pull ghcr.io/mbv/ai-mesh-node-reboot-scheduler:latest

# Run the container
docker run -d \
  --name router-reboot \
  --restart unless-stopped \
  -e ROUTER_IP=192.168.1.1 \
  -e ROUTER_USERNAME=admin \
  -e ROUTER_PASSWORD=your_password \
  -e DEVICE_MAC=AA:BB:CC:DD:EE:FF \
  -e TIMEZONE=Europe/Warsaw \
  -e CRON_SCHEDULE="0 3 * * *" \
  ghcr.io/mbv/ai-mesh-node-reboot-scheduler:latest
```

### Building Locally

1. Build the Docker image:
```bash
docker build -t router-reboot-scheduler .
```

2. Run the container:
```bash
docker run -d \
  --name router-reboot \
  --restart unless-stopped \
  -e ROUTER_IP=192.168.1.1 \
  -e ROUTER_USERNAME=admin \
  -e ROUTER_PASSWORD=your_password \
  -e DEVICE_MAC=AA:BB:CC:DD:EE:FF \
  -e TIMEZONE=Europe/Warsaw \
  -e CRON_SCHEDULE="0 3 * * *" \
  router-reboot-scheduler
```

### Using Docker Compose

Create a `docker-compose.yml` file:

```yaml
version: '3.8'

services:
  router-reboot:
    build: .
    container_name: router-reboot
    restart: unless-stopped
    environment:
      - ROUTER_IP=192.168.1.1
      - ROUTER_USERNAME=admin
      - ROUTER_PASSWORD=your_password
      - DEVICE_MAC=AA:BB:CC:DD:EE:FF
      - TIMEZONE=Europe/Warsaw
      - CRON_SCHEDULE=0 3 * * *
```

Then run:
```bash
docker-compose up -d
```

### Testing

To test the reboot functionality immediately:

```bash
docker run --rm \
  -e ROUTER_IP=192.168.1.1 \
  -e ROUTER_USERNAME=admin \
  -e ROUTER_PASSWORD=your_password \
  -e DEVICE_MAC=AA:BB:CC:DD:EE:FF \
  -e IMMEDIATE_REBOOT=true \
  router-reboot-scheduler
```

### Running Locally (without Docker)

1. Install Rust (if not already installed):
```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

2. Build the application:
```bash
cargo build --release
```

3. Set environment variables and run:
```bash
export ROUTER_IP=192.168.1.1
export ROUTER_USERNAME=admin
export ROUTER_PASSWORD=your_password
export DEVICE_MAC=AA:BB:CC:DD:EE:FF
export TIMEZONE=Europe/Warsaw
export CRON_SCHEDULE="0 3 * * *"
./target/release/reboot_scheduler
```

## How It Works

1. **Authentication Flow**:
   - Requests a nonce from `/get_Nonce.cgi`
   - Generates a SHA256 hash: `sha256(username:nonce:password:cnonce)`
   - Authenticates via `/login_v2.cgi` and receives an `asus_token` cookie

2. **Reboot Flow**:
   - Sends a POST request to `/applyapp.cgi` with the device MAC and reboot action
   - Uses the authentication token from the login step

3. **Scheduling**:
   - Uses APScheduler with cron triggers for flexible scheduling
   - Supports standard cron format for defining reboot times
   - Timezone-aware scheduling based on the `TIMEZONE` environment variable

## Logs

The script logs all operations to stdout. To view logs from a running container:

```bash
docker logs router-reboot
```

Or follow logs in real-time:

```bash
docker logs -f router-reboot
```

## Security Notes

- Never commit credentials to version control
- Use Docker secrets or environment variable files for production
- Ensure the container has network access to the router
- The script uses HTTP (not HTTPS) - ensure your network is secure

## Timezone and Scheduling

The timezone and schedule are fully configurable via environment variables:

- **TIMEZONE**: Set the timezone for scheduling (e.g., `Europe/Warsaw`, `America/New_York`, `Asia/Tokyo`)
- **CRON_SCHEDULE**: Set the schedule using cron format (e.g., `0 3 * * *` for 3:00 AM daily)

The system timezone (`TZ`) should match the `TIMEZONE` environment variable for consistency. Both are set via environment variables in docker-compose.yml.

### Common Timezone Examples

- `Europe/Warsaw` - Central European Time
- `America/New_York` - Eastern Time
- `America/Los_Angeles` - Pacific Time
- `Asia/Tokyo` - Japan Standard Time
- `UTC` - Coordinated Universal Time

## Troubleshooting

- **Authentication fails**: Verify username and password are correct
- **Reboot doesn't work**: Check that the device MAC address is correct
- **Can't connect to router**: Ensure the container can reach the router IP
- **Timezone issues**: Ensure both `TZ` and `TIMEZONE` environment variables are set to the same timezone
- **Schedule not working**: Verify the cron expression format is correct (5 fields: minute hour day month day_of_week)
