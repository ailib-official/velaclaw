#!/usr/bin/env bash
# VelaClaw Remote Server Bootstrap Script
# 
# This script prepares a remote server for VelaClaw deployment by setting up
# the necessary permissions, directories, and sudoers configuration.
#
# Usage: bash <(curl -s https://your-server/bootstrap.sh)
#        Or: scp bootstrap.sh remote-server:/tmp/ && ssh remote-server 'bash /tmp/bootstrap.sh'
#
# This script should be run ONCE on each remote server before first deployment.

set -e

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

echo -e "${GREEN}=== VelaClaw Remote Server Bootstrap ===${NC}"
echo ""

# Function to print info
info() {
    echo -e "${GREEN}[INFO]${NC} $1"
}

# Function to print warning
warn() {
    echo -e "${YELLOW}[WARN]${NC} $1"
}

# Function to print error and exit
error() {
    echo -e "${RED}[ERROR]${NC} $1"
    exit 1
}

# Check if running as root
if [[ $EUID -eq 0 ]]; then
    warn "Running as root. Some operations will use sudo for deploy user."
else
    warn "Not running as root. Some operations may require sudo access."
fi

# 1. Create deploy user (if needed)
if ! id deploy &>/dev/null; then
    info "Creating 'deploy' user..."
    sudo useradd -m -s /bin/bash deploy
    info "✓ User 'deploy' created"
else
    info "✓ User 'deploy' already exists"
fi

# 2. Create working directory
info "Setting up working directory..."
sudo mkdir -p /opt/velaclaw
sudo chown -R deploy:deploy /opt/velaclaw
info "✓ Working directory /opt/velaclaw ready"

# 3. Create logs directory
sudo mkdir -p /opt/velaclaw/logs
sudo chown -R deploy:deploy /opt/velaclaw/logs
info "✓ Logs directory ready"

# 4. Create remote binary file (placeholder)
info "Creating placeholder for remote binary..."
if [[ ! -f /usr/local/bin/velaclaw ]]; then
    sudo touch /usr/local/bin/velaclaw
    sudo chown deploy:deploy /usr/local/bin/velaclaw
    sudo chmod 755 /usr/local/bin/velaclaw
    info "✓ Remote binary placeholder created at /usr/local/bin/velaclaw"
else
    info "✓ Remote binary file already exists at /usr/local/bin/velaclaw"
    info "  (will be overwritten during deployment)"
fi

# 5. Configure sudoers for deploy user (systemd mode)
info "Configuring sudoers for systemctl commands..."
SUDOERS_FILE="/etc/sudoers.d/deployer-velaclaw"

sudo tee "$SUDOERS_FILE" > /dev/null <<EOF
# VelaClaw deployment sudoers for 'deploy' user
# Allows deploy user to manage velaclaw systemd service without password

# Systemd commands
deploy ALL=(ALL) NOPASSWD: /usr/bin/systemctl daemon-reload, \
    /usr/bin/systemctl enable velaclaw, \
    /usr/bin/systemctl start velaclaw, \
    /usr/bin/systemctl stop velaclaw, \
    /usr/bin/systemctl restart velaclaw

# Directory creation/ownership (for working directory)
deploy ALL=(ALL) NOPASSWD: /bin/mkdir -p /opt/velaclaw, \
    /bin/mkdir -p /opt/velaclaw/logs, \
    /bin/chown -R deploy:deploy /opt/velaclaw, \
    /bin/chown -R deploy:deploy /usr/local/bin/velaclaw

# Binary installation
deploy ALL=(ALL) NOPASSWD: /usr/bin/touch /usr/local/bin/velaclaw, \
    /bin/chown deploy:deploy /usr/local/bin/velaclaw, \
    /usr/bin/chmod 755 /usr/local/bin/velaclaw
EOF

sudo chmod 0440 "$SUDOERS_FILE"
info "✓ Sudoers configured at $SUDOERS_FILE"

# 6. Docker group (for Docker mode)
if command -v docker &>/dev/null; then
    info "Docker detected. Adding 'deploy' user to docker group..."
    sudo usermod -aG docker deploy
    info "✓ Added to docker group (may need logout/login for changes to take effect)"
    warn "  Note: Docker group changes require re-login: ssh deploy@host"
else
    info "Docker not detected (will skip docker group setup)"
fi

# 7. Systemd service file (for systemd mode)
info "Creating systemd service file for velaclaw..."
SYSTEMD_FILE="/etc/systemd/system/velaclaw.service"

sudo tee "$SYSTEMD_FILE" > /dev/null <<EOF
[Unit]
Description=VelaClaw AI Agent
After=network.target

[Service]
Type=simple
User=deploy
WorkingDirectory=/opt/velaclaw
ExecStart=/usr/local/bin/velaclaw
Restart=on-failure
RestartSec=10
StandardOutput=attach:/opt/velaclaw/logs/velaclaw.log
StandardError=attach:/opt/velaclaw/logs/velaclaw-error.log

[Install]
WantedBy=multi-user.target
EOF

info "✓ Systemd service file created at $SYSTEMD_FILE"

# 8. Reload systemd daemon (if running systemd)
if command -v systemctl &>/dev/null && systemctl is-system-running; then
    info "Reloading systemd daemon..."
    sudo systemctl daemon-reload
    info "✓ Systemd daemon reloaded"
else
    warn "systemd not running or not detected (will reload during deployment)"
fi

# 9. Summary
echo ""
echo -e "${GREEN}=== Bootstrap Complete ===${NC}"
echo ""
info "The remote server is now ready for VelaClaw deployment!"
echo ""
echo -e "${YELLOW}Next steps:${NC}"
echo "1. Run 'velaclaw deploy validate --server <your-server-id>' to verify permissions"
echo "2. Run 'velaclaw deploy deploy --server <your-server-id>' to deploy"
echo ""
echo -e "${GREEN}Deployment modes supported:${NC}"
echo "  - Direct:  Binary deployment (use sudo=true if needed)"
echo "  - Docker:  Container deployment (use sudo=true if not in docker group yet)"
echo "  - Systemd: Service deployment (sudo required, already configured)"
echo ""
echo -e "${YELLOW}Important notes:${NC}"
echo "  - For systemd/systemctl: sudo is configured in $SUDOERS_FILE"
echo "  - For Docker: Re-login may be needed after adding to docker group"
echo "  - First deployment may be slower as files are copied"
echo ""

# 10. Create a marker file that bootstrap was run
echo "bootstrap=$(date -Iseconds)" | sudo tee /opt/velaclaw/.bootstrap-complete > /dev/null
sudo chown deploy:deploy /opt/velaclaw/.bootstrap-complete
info "✓ Marker file created at /opt/velaclaw/.bootstrap-complete"
echo ""
echo -e "${GREEN}Bootstrap script completed successfully!${NC}"
