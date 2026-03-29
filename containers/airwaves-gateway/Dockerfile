# Stage 1: Build the control app
FROM node:22-alpine AS app-builder

WORKDIR /app

# Copy control app source (mounted from airwaves-os-control repo)
# In CI, this is cloned from github.com/airframesio/airwaves-os-control
COPY control-app/ .

RUN npm ci
RUN npm run build

# Stage 2: Nginx gateway
FROM nginx:1-alpine

LABEL org.opencontainers.image.source="https://github.com/airframesio/airwaves-os"
LABEL org.opencontainers.image.description="Airwaves OS Gateway - Reverse proxy and web management app"

# Install curl for healthcheck
RUN apk add --no-cache curl

# Remove default nginx config
RUN rm /etc/nginx/conf.d/default.conf

# Copy our configuration
COPY nginx.conf /etc/nginx/nginx.conf
COPY default.conf /etc/nginx/conf.d/default.conf

# Copy the built control app (SPA)
COPY --from=app-builder /app/dist/public/ /usr/share/nginx/html/

# Fallback: if no control app was built, use default landing page
COPY html/ /usr/share/nginx/html-fallback/

EXPOSE 80 443

HEALTHCHECK --interval=30s --timeout=5s --retries=3 \
    CMD curl -f http://localhost/health || exit 1
