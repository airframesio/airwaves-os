FROM nginx:1-alpine

LABEL org.opencontainers.image.source="https://github.com/airframesio/airwaves-os"
LABEL org.opencontainers.image.description="Airwaves OS Gateway - Reverse proxy and web server"

# Install curl for healthcheck
RUN apk add --no-cache curl

# Remove default nginx config
RUN rm /etc/nginx/conf.d/default.conf

# Copy our configuration
COPY nginx.conf /etc/nginx/nginx.conf
COPY default.conf /etc/nginx/conf.d/default.conf

# Copy default landing page (replaced by web management app later)
COPY html/ /usr/share/nginx/html/

EXPOSE 80 443

HEALTHCHECK --interval=30s --timeout=5s --retries=3 \
    CMD curl -f http://localhost/health || exit 1
