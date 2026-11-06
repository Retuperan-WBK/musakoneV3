# Build stage
FROM oven/bun:1.3.6-alpine AS builder

WORKDIR /app

# Copy package files
COPY package.json bun.lock* ./

# Install dependencies
RUN bun install --frozen-lockfile

# Copy source code
COPY . .

# Build the application
RUN bun run build

# Production stage - Use Bun native server
FROM oven/bun:1.3.6-alpine

WORKDIR /app

# Reuse the build output; the native static server needs no npm dependencies.
COPY --from=builder /app/dist ./dist
COPY server.ts ./server.ts

# Expose port for Bun static server
EXPOSE 3000

# Start Bun static server (TypeScript entrypoint)
CMD ["bun", "run", "server.ts"]
