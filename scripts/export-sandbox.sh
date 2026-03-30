#!/usr/bin/env bash
set -euo pipefail

# ============================================================================
# Export current Claude Code setup to a Docker AI Sandbox template
#
# What it does:
#   1. Assembles a build context with global Claude config, skills, and tool sources
#   2. Generates a Dockerfile based on docker/sandbox-templates:claude-code
#   3. Builds the template image
#
# Usage:
#   ./scripts/export-sandbox.sh              # build only
#   ./scripts/export-sandbox.sh --run        # build and launch sandbox
#
# Prerequisites:
#   - Docker Desktop 4.58+ with sandbox support
#   - ANTHROPIC_API_KEY set in shell config (not just current session)
# ============================================================================

IMAGE_NAME="koupang-sandbox:latest"
BUILD_DIR="/tmp/koupang-sandbox-build"
CLAUDE_HOME="$HOME/.claude"
PROJECT_DIR="$(cd "$(dirname "$0")/.." && pwd)"

echo "=== Koupang Docker Sandbox Export ==="
echo "Project: $PROJECT_DIR"
echo "Image:   $IMAGE_NAME"
echo ""

# Clean and create build context
rm -rf "$BUILD_DIR"
mkdir -p "$BUILD_DIR"

# --------------------------------------------------------------------------
# 1. Global Claude config (~/.claude/ -> image /home/agent/.claude/)
# --------------------------------------------------------------------------
echo "[1/5] Copying global Claude config..."

mkdir -p "$BUILD_DIR/claude-home"

# Global CLAUDE.md (working style instructions)
cp "$CLAUDE_HOME/CLAUDE.md" "$BUILD_DIR/claude-home/" 2>/dev/null || true

# Global settings (permissions, hooks, plugins)
# NOTE: Sandbox runs --dangerously-skip-permissions, so permissions are
# informational only. Hooks and plugin config still matter.
cp "$CLAUDE_HOME/settings.json" "$BUILD_DIR/claude-home/" 2>/dev/null || true

# --------------------------------------------------------------------------
# 2. Skills (~/.claude/skills/ -> image /home/agent/.claude/skills/)
# --------------------------------------------------------------------------
echo "[2/5] Copying skills..."

mkdir -p "$BUILD_DIR/claude-home/skills"

# Copy all skill directories (SKILL.md + references/)
# Exclude: .git dirs, library catalog (rebuilt), research binary (rebuilt)
rsync -a \
  --exclude='.git' \
  --exclude='library/.git' \
  --exclude='research/bin/' \
  --exclude='research/src/' \
  "$CLAUDE_HOME/skills/" "$BUILD_DIR/claude-home/skills/"

# Copy research tool source (will be compiled for linux in Docker build)
mkdir -p "$BUILD_DIR/research-src"
cp "$CLAUDE_HOME/skills/research/src/go.mod" "$BUILD_DIR/research-src/"
cp "$CLAUDE_HOME/skills/research/src/main.go" "$BUILD_DIR/research-src/"

# --------------------------------------------------------------------------
# 3. CLI tool sources (compiled for linux during Docker build)
# --------------------------------------------------------------------------
echo "[3/5] Preparing CLI tool sources..."

# Filament (fl) — Rust workspace, compiled from git
# Jujo — Rust crate, compiled from git
# sqlx-cli — installed via cargo install
# These are all compiled inside the Docker build for linux target.

# --------------------------------------------------------------------------
# 4. Generate Dockerfile
# --------------------------------------------------------------------------
echo "[4/5] Generating Dockerfile..."

cat > "$BUILD_DIR/Dockerfile" << 'DOCKERFILE'
# ==========================================================================
# Koupang Development Sandbox
# Base: docker/sandbox-templates:claude-code (Ubuntu + Claude Code agent)
# Adds: Rust toolchain, project CLI tools, Claude skills & config
# ==========================================================================
FROM docker/sandbox-templates:claude-code

USER root

# -- System packages needed for Rust compilation and project deps -----------
RUN apt-get update && apt-get install -y --no-install-recommends \
    build-essential \
    pkg-config \
    libssl-dev \
    libpq-dev \
    cmake \
    && rm -rf /var/lib/apt/lists/*

# -- Rust toolchain ---------------------------------------------------------
ENV RUSTUP_HOME=/usr/local/rustup \
    CARGO_HOME=/usr/local/cargo \
    PATH=/usr/local/cargo/bin:$PATH

RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | \
    sh -s -- -y --default-toolchain stable --profile default && \
    rustup component add clippy rustfmt && \
    chmod -R a+rw $RUSTUP_HOME $CARGO_HOME

# -- sqlx-cli (database migrations) ----------------------------------------
RUN cargo install sqlx-cli --no-default-features --features postgres,rustls

# -- Filament CLI (fl) — task/knowledge management -------------------------
RUN cargo install --git https://github.com/JYC11/filament.git \
    --bin fl filament-cli

# -- Jujo CLI — code generator ---------------------------------------------
RUN cargo install --git https://github.com/JYC11/jujo.git jujo

# -- Research tool (Go binary) ---------------------------------------------
COPY research-src/ /tmp/research-build/
RUN cd /tmp/research-build && \
    go build -o /usr/local/bin/research . && \
    rm -rf /tmp/research-build

# -- Global Claude config & skills -----------------------------------------
COPY claude-home/ /home/agent/.claude/

# Create research binary directory and link
RUN mkdir -p /home/agent/.claude/skills/research/bin && \
    ln -sf /usr/local/bin/research /home/agent/.claude/skills/research/bin/research

# -- Fix ownership ----------------------------------------------------------
RUN chown -R agent:agent /home/agent/.claude

# -- Clean cargo build cache to reduce image size --------------------------
RUN rm -rf $CARGO_HOME/registry/cache $CARGO_HOME/registry/src \
    $CARGO_HOME/git/checkouts

USER agent

# Verify tools are available
RUN rustc --version && cargo --version && \
    fl --version && jujo --version && sqlx --version && \
    research --help > /dev/null 2>&1 || true
DOCKERFILE

# --------------------------------------------------------------------------
# 5. Build
# --------------------------------------------------------------------------
echo "[5/5] Building Docker image..."
echo ""

docker build -t "$IMAGE_NAME" "$BUILD_DIR"

echo ""
echo "=== Build complete ==="
echo "Image: $IMAGE_NAME"
echo ""
echo "Usage:"
echo "  docker sandbox run -t $IMAGE_NAME claude $PROJECT_DIR"
echo ""
echo "Or with a prompt:"
echo "  docker sandbox run -t $IMAGE_NAME claude $PROJECT_DIR -- \"your prompt here\""
echo ""
echo "Notes:"
echo "  - ANTHROPIC_API_KEY must be set in ~/.zshrc (not just current shell)"
echo "  - Restart Docker Desktop after setting the key"
echo "  - Project files (.claude/, CLAUDE.md, STYLE.md) are bind-mounted automatically"
echo "  - Sandbox runs --dangerously-skip-permissions (permission settings are ignored)"
echo "  - Plugins (rust-skills, rust-analyzer-lsp) need manual install inside sandbox"
echo "    Run: claude plugins install rust-skills@rust-skills"

if [[ "${1:-}" == "--run" ]]; then
  echo ""
  echo "=== Launching sandbox ==="
  docker sandbox run -t "$IMAGE_NAME" claude "$PROJECT_DIR"
fi
