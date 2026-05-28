# Stage 1: Build the control app (if source is provided)
FROM node:22-alpine AS app-builder

WORKDIR /app

# Copy control app source if it exists. If not, the build stage
# produces nothing and the fallback landing page is used.
COPY control-app* ./control-app-check/

RUN if [ -f control-app-check/package.json ]; then \
      cp -a control-app-check/. . && \
      rm -rf control-app-check && \
      npm ci && \
      npm run build; \
    else \
      mkdir -p dist/public && \
      echo "no control app" > dist/public/.skip; \
    fi

# Stage 2: Nginx gateway
FROM nginx:1-alpine

# Control-app version, passed by CI from the cloned control-app package.json.
# Surfaced at /version.json so the manager can report the deployed control-app
# version and detect updates.
ARG CONTROL_APP_VERSION=unknown

LABEL org.opencontainers.image.source="https://github.com/airframesio/airwaves-os"
LABEL org.opencontainers.image.description="Airwaves OS Gateway - Reverse proxy and web management app"
LABEL io.airwaves.control-app-version="${CONTROL_APP_VERSION}"

RUN apk add --no-cache curl

RUN rm /etc/nginx/conf.d/default.conf

COPY nginx.conf /etc/nginx/nginx.conf
COPY default.conf /etc/nginx/conf.d/default.conf

# Copy the default landing page first
COPY html/ /usr/share/nginx/html/

# Overlay with built control app if it was compiled
COPY --from=app-builder /app/dist/public/ /usr/share/nginx/html/

# Remove the skip marker if it was the fallback
RUN rm -f /usr/share/nginx/html/.skip

# Stamp the deployed control-app version for the manager to read.
RUN printf '{"name":"airwaves-os-control","version":"%s"}\n' "${CONTROL_APP_VERSION}" \
    > /usr/share/nginx/html/version.json

EXPOSE 80 443

HEALTHCHECK --interval=30s --timeout=5s --retries=3 \
    CMD curl -f http://localhost/health || exit 1
